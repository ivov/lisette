use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::abi::is_tagged_shape_fn_value;
use crate::abi::transition::lower_arg_to_tagged;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::{CapturePolicy, StagedExpression};
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::utils::observable_after_mutation;
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

/// Folds `f(leading, spread...)` into `f(append([]T{leading}, spread...)...)` — Go rejects the former.
#[derive(Clone)]
pub(crate) struct VariadicCombine {
    pub element_ty: Type,
    /// EmittedExpr-value index where variadic-feeding args begin.
    pub fixed_count: usize,
}

impl Planner<'_> {
    pub(crate) fn stage_or_capture(
        &mut self,
        expression: &Expression,
        prefix: &str,
        fx: &mut EmitEffects,
    ) -> StagedExpression {
        if matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        ) {
            return self.stage_operand(expression, ExpressionContext::value(), fx);
        }

        let staged = self.stage_operand(expression, ExpressionContext::value(), fx);
        let mut setup = staged.setup;
        let temp_var = self.hoist_tmp_value_statement(&mut setup, prefix, &staged.value);
        StagedExpression {
            setup,
            value: temp_var,
            capture: CapturePolicy::Never,
            non_literal: false,
        }
    }

    /// Pin a staged operand's value into a temp so it evaluates before any
    /// later sibling.
    pub(crate) fn pin_staged(&mut self, staged: &mut StagedExpression, prefix: &str) {
        let value = std::mem::take(&mut staged.value);
        let tmp = self.hoist_tmp_value_statement(&mut staged.setup, prefix, &value);
        staged.value = tmp;
        staged.capture = CapturePolicy::Never;
    }

    pub(crate) fn emit_force_capture(
        &mut self,
        output: &mut String,
        expression: &Expression,
        prefix: &str,
        fx: &mut EmitEffects,
    ) -> String {
        if !observable_after_mutation(expression) {
            return self.emit_operand(output, expression, ExpressionContext::value(), fx);
        }

        let temp_var = self.fresh_var(Some(prefix));
        self.declare(&temp_var);
        let expression_string =
            self.emit_composite_value(output, expression, ExpressionContext::value(), fx);
        write_line!(output, "{} := {}", temp_var, expression_string);
        temp_var
    }

    /// Plan an expression and capture its typed setup and value.
    pub(crate) fn stage_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> StagedExpression {
        let plan = self.plan_operand(expression, ctx, fx);
        StagedExpression::from_plan(plan, expression)
    }

    /// Stage an expression as a composite value, capturing typed setup.
    pub(crate) fn stage_composite(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> StagedExpression {
        let (setup, value) = self.lower_composite_value(expression, ctx, fx);
        StagedExpression::from_typed_setup(setup, value, expression)
    }

    /// Suppresses the Go-fn identity short-circuit when the formal param
    /// is function-typed (prelude generic callbacks reject multi-return).
    pub(crate) fn stage_prelude_arg(
        &mut self,
        expression: &Expression,
        param_ty: Option<&syntax::types::Type>,
        fx: &mut EmitEffects,
    ) -> StagedExpression {
        let suppress =
            param_ty.is_some_and(|p| matches!(p.unwrap_forall(), syntax::types::Type::Function(_)));
        let arg_ctx = ExpressionContext::value().with_forced_tagged_go_function(suppress);
        let staged = self.stage_composite(expression, arg_ctx, fx);

        if suppress {
            // `try_lower_arg_to_tagged` mutates a `String` setup; render the
            // staged setup down for it, then re-wrap on the way out.
            let mut setup = Renderer.render_setup(&staged.setup);
            if let Some(tagged) =
                self.try_lower_arg_to_tagged(&mut setup, expression, &staged.value, param_ty, fx)
            {
                return StagedExpression::new(setup, tagged, expression);
            }
            return StagedExpression::new(setup, staged.value, expression);
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
        fx: &mut EmitEffects,
    ) -> Option<String> {
        self.detect_lower_arg_to_tagged(arg, param_ty)?;
        Some(self.emit_lower_arg_to_tagged(output, value, param_ty.unwrap(), fx))
    }

    /// Detect whether a tagged-Go lowering applies. Pure: no emission.
    pub(crate) fn detect_lower_arg_to_tagged(
        &self,
        arg: &Expression,
        param_ty: Option<&Type>,
    ) -> Option<()> {
        if matches!(arg.unwrap_parens(), Expression::Lambda { .. }) {
            return None;
        }
        if is_tagged_shape_fn_value(arg) {
            return None;
        }
        if self.classify_go_fn_value(arg).is_some() {
            return None;
        }
        let param_ty = param_ty?;
        let Type::Function(f) = param_ty.unwrap_forall() else {
            return None;
        };
        self.classify_direct_emission(&f.return_type)?;
        Some(())
    }

    pub(crate) fn emit_lower_arg_to_tagged(
        &mut self,
        output: &mut String,
        value: &str,
        param_ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let cb_var = self.hoist_tmp_value(output, "cb", value);
        lower_arg_to_tagged(self, output, &cb_var, param_ty, fx)
    }

    pub(crate) fn stage_native_method_args(
        &mut self,
        function: &Expression,
        args: &[Expression],
        fx: &mut EmitEffects,
    ) -> Vec<StagedExpression> {
        let fn_ty = function.get_type();
        let formal_params: &[syntax::types::Type] = match fn_ty.unwrap_forall() {
            syntax::types::Type::Function(f) => &f.params,
            _ => &[],
        };
        args.iter()
            .enumerate()
            .map(|(i, arg)| self.stage_prelude_arg(arg, formal_params.get(i), fx))
            .collect()
    }

    /// Post-staging fix-up for the spread slot: optional `any`-wrap, then
    /// either `append([]T{leading...}, spread...)...` or plain `value...`.
    pub(crate) fn finalize_spread_stage(
        &mut self,
        values: &mut Vec<String>,
        spread_index: usize,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
        fx: &mut EmitEffects,
    ) {
        if wrap_to_any {
            fx.require_stdlib();
            values[spread_index] = format!(
                "{}.SliceToAny({})",
                go_name::GO_STDLIB_PKG,
                values[spread_index]
            );
        }
        match combine {
            Some(c) if spread_index > c.fixed_count => {
                let element_go = self.go_type_string(&c.element_ty, fx);
                let leading = values[c.fixed_count..spread_index].join(", ");
                let spread_value = &values[spread_index];
                let combined = format!("append([]{element_go}{{{leading}}}, {spread_value}...)...");
                values.splice(c.fixed_count..=spread_index, std::iter::once(combined));
            }
            _ => values[spread_index].push_str("..."),
        }
    }

    /// Sequence N staged emissions preserving left-to-right eval order,
    /// accumulating setup into a `Vec<LoweredStatement>` so a value plan can
    /// carry it as structured setup. A later sibling with setup forces an
    /// earlier inline-but-observable value to be captured to a `TempBind`.
    /// Returns `(setup_statements, values)`.
    pub(crate) fn sequence_structured(
        &mut self,
        mut stages: Vec<StagedExpression>,
        prefix: &str,
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let eager = self.function_state.eager_operand_capture();
        if !eager && stages.iter().all(|s| s.setup.is_empty()) {
            return (Vec::new(), stages.into_iter().map(|s| s.value).collect());
        }

        let mut setup: Vec<LoweredStatement> = Vec::new();
        let mut results = Vec::with_capacity(stages.len());
        for i in 0..stages.len() {
            let later_has_setup = stages[i + 1..].iter().any(|s| !s.setup.is_empty());
            let s_capture = stages[i].capture;
            let s_non_literal = stages[i].non_literal;
            let s_value = std::mem::take(&mut stages[i].value);
            let s_setup = std::mem::take(&mut stages[i].setup);

            setup.extend(s_setup);

            let capture_for_later =
                later_has_setup && matches!(s_capture, CapturePolicy::IfLaterSetup);
            if capture_for_later || (eager && s_non_literal) {
                let tmp = self.fresh_var(Some(prefix));
                self.declare(&tmp);
                setup.push(LoweredStatement::TempBind {
                    name: tmp.clone(),
                    value: s_value,
                });
                results.push(tmp);
            } else {
                results.push(s_value);
            }
        }
        (setup, results)
    }

    /// Structured counterpart of `sequence_with_spread`: stages the spread as a
    /// sibling, sequences via `sequence_structured` (so setup is structured
    /// statements, not flushed to a buffer), then applies the spread fix-up to
    /// the value slot. Returns `(setup_statements, values)`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sequence_with_spread_structured(
        &mut self,
        mut stages: Vec<StagedExpression>,
        spread: Option<&Expression>,
        wrap_to_any: bool,
        prefix: &str,
        combine: Option<VariadicCombine>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let spread_index = spread.map(|s| {
            stages.push(self.stage_operand(s, ExpressionContext::value(), fx));
            stages.len() - 1
        });
        let (setup, mut values) = self.sequence_structured(stages, prefix);
        if let Some(i) = spread_index {
            self.finalize_spread_stage(&mut values, i, wrap_to_any, combine, fx);
        }
        (setup, values)
    }
}
