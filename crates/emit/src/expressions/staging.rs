use crate::Emitter;
use crate::expressions::context::ExpressionContext;
use crate::expressions::emission::{CapturePolicy, EmittedExpression};
use crate::names::go_name;
use crate::utils::observable_after_mutation;
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

/// Folds `f(leading, spread...)` into `f(append([]T{leading}, spread...)...)` — Go rejects the former.
#[derive(Clone)]
pub(crate) struct VariadicCombine {
    pub elem_ty: Type,
    /// EmittedExpr-value index where variadic-feeding args begin.
    pub fixed_count: usize,
}

impl Emitter<'_> {
    pub(crate) fn stage_or_capture(
        &mut self,
        expression: &Expression,
        prefix: &str,
    ) -> EmittedExpression {
        if matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        ) {
            return self.stage_operand(expression, ExpressionContext::value());
        }

        let mut setup = String::new();
        let value_expr = self.emit_operand(&mut setup, expression, ExpressionContext::value());
        let temp_var = self.hoist_tmp_value(&mut setup, prefix, &value_expr);
        EmittedExpression::new(setup, temp_var, expression)
    }

    pub(crate) fn emit_force_capture(
        &mut self,
        output: &mut String,
        expression: &Expression,
        prefix: &str,
    ) -> String {
        if !observable_after_mutation(expression) {
            return self.emit_operand(output, expression, ExpressionContext::value());
        }

        let temp_var = self.fresh_var(Some(prefix));
        self.declare(&temp_var);
        let expression_string =
            self.emit_composite_value(output, expression, ExpressionContext::value());
        write_line!(output, "{} := {}", temp_var, expression_string);
        temp_var
    }

    /// Emit an expression to a separate buffer, capturing setup and value.
    pub(crate) fn stage_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> EmittedExpression {
        let mut setup = String::new();
        let value = self.emit_operand(&mut setup, expression, ctx);
        EmittedExpression::new(setup, value, expression)
    }

    /// Emit an expression as a composite value to a separate buffer.
    pub(crate) fn stage_composite(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> EmittedExpression {
        let mut setup = String::new();
        let value = self.emit_composite_value(&mut setup, expression, ctx);
        EmittedExpression::new(setup, value, expression)
    }

    /// Suppresses the Go-fn identity short-circuit when the formal param
    /// is function-typed (prelude generic callbacks reject multi-return).
    pub(crate) fn stage_prelude_arg(
        &mut self,
        expression: &Expression,
        param_ty: Option<&syntax::types::Type>,
    ) -> EmittedExpression {
        let suppress = param_ty
            .is_some_and(|p| matches!(p.unwrap_forall(), syntax::types::Type::Function { .. }));
        let arg_ctx = ExpressionContext::value().with_forced_tagged_go_function(suppress);
        let staged = self.stage_composite(expression, arg_ctx);

        if suppress {
            let mut setup = staged.setup;
            if let Some(tagged) =
                self.try_lower_arg_to_tagged(&mut setup, expression, &staged.value, param_ty)
            {
                return EmittedExpression::new(setup, tagged, expression);
            }
            return EmittedExpression::new(setup, staged.value, expression);
        }

        staged
    }

    /// Adapt a lowered-ABI Lisette callback to tagged shape when the callee
    /// (prelude generic or otherwise) declares a function param whose return
    /// classifies for direct (lowered) emission. Returns `None` when the arg
    /// already has tagged shape, is a lambda literal, or is a Go fn value.
    pub(crate) fn try_lower_arg_to_tagged(
        &mut self,
        output: &mut String,
        arg: &Expression,
        value: &str,
        param_ty: Option<&Type>,
    ) -> Option<String> {
        if matches!(arg.unwrap_parens(), Expression::Lambda { .. }) {
            return None;
        }
        if Self::is_tagged_shape_fn_value(arg) {
            return None;
        }
        if self.classify_go_fn_value(arg).is_some() {
            return None;
        }
        let param_ty = param_ty?;
        let Type::Function { return_type, .. } = param_ty.unwrap_forall() else {
            return None;
        };
        self.classify_direct_emission(return_type)?;
        let cb_var = self.hoist_tmp_value(output, "cb", value);
        Some(crate::types::abi_transition::lower_arg_to_tagged(
            self, output, &cb_var, param_ty,
        ))
    }

    pub(crate) fn stage_native_method_args(
        &mut self,
        function: &Expression,
        args: &[Expression],
    ) -> Vec<EmittedExpression> {
        let fn_ty = function.get_type();
        let formal_params: &[syntax::types::Type] = match fn_ty.unwrap_forall() {
            syntax::types::Type::Function { params, .. } => params,
            _ => &[],
        };
        args.iter()
            .enumerate()
            .map(|(i, arg)| self.stage_prelude_arg(arg, formal_params.get(i)))
            .collect()
    }

    /// Like `sequence`, but also stages the spread as a sibling (so its
    /// setup participates in eval-order) and appends `...` to its value.
    /// When `combine` is `Some`, leading args feeding the variadic are folded
    /// with the spread into a single `append([]T{...}, spread...)...` value
    /// so the resulting Go is well-formed.
    pub(crate) fn sequence_with_spread(
        &mut self,
        output: &mut String,
        mut stages: Vec<EmittedExpression>,
        spread: Option<&Expression>,
        wrap_to_any: bool,
        prefix: &str,
        combine: Option<VariadicCombine>,
    ) -> Vec<String> {
        let spread_idx = spread.map(|s| {
            stages.push(self.stage_operand(s, ExpressionContext::value()));
            stages.len() - 1
        });
        let mut values = self.sequence(output, stages, prefix);
        if let Some(i) = spread_idx {
            self.finalize_spread_stage(&mut values, i, wrap_to_any, combine);
        }
        values
    }

    /// Post-staging fix-up for the spread slot: optional `any`-wrap, then
    /// either `append([]T{leading...}, spread...)...` or plain `value...`.
    pub(crate) fn finalize_spread_stage(
        &mut self,
        values: &mut Vec<String>,
        spread_idx: usize,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
    ) {
        if wrap_to_any {
            self.requirements.require_stdlib();
            values[spread_idx] = format!(
                "{}.SliceToAny({})",
                go_name::GO_STDLIB_PKG,
                values[spread_idx]
            );
        }
        match combine {
            Some(c) if spread_idx > c.fixed_count => {
                let elem_go = self.go_type_as_string(&c.elem_ty);
                let leading = values[c.fixed_count..spread_idx].join(", ");
                let spread_value = &values[spread_idx];
                let combined = format!("append([]{elem_go}{{{leading}}}, {spread_value}...)...");
                values.splice(c.fixed_count..=spread_idx, std::iter::once(combined));
            }
            _ => values[spread_idx].push_str("..."),
        }
    }

    /// Sequence N staged emissions preserving left-to-right eval order.
    ///
    /// When a later sibling produces setup statements (temp vars from if/match/block
    /// used as values), earlier siblings that contain calls are captured to temp vars
    /// to prevent the setup from running before the earlier call.
    pub(crate) fn sequence(
        &mut self,
        output: &mut String,
        stages: Vec<EmittedExpression>,
        prefix: &str,
    ) -> Vec<String> {
        // Fast path: when no element produces setup, just move the values out.
        if stages.iter().all(|s| s.setup.is_empty()) {
            return stages.into_iter().map(|s| s.value).collect();
        }

        let mut results = Vec::with_capacity(stages.len());
        for (i, s) in stages.iter().enumerate() {
            let later_has_setup = stages[i + 1..].iter().any(|s| !s.setup.is_empty());

            output.push_str(&s.setup);

            if later_has_setup && matches!(s.capture, CapturePolicy::IfLaterSetup) {
                let tmp = self.hoist_tmp_value(output, prefix, &s.value);
                results.push(tmp);
            } else {
                results.push(s.value.clone());
            }
        }
        results
    }
}
