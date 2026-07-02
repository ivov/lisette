use crate::Planner;
use crate::abi::is_tagged_shape_fn_value;
use crate::abi::transition::lower_arg_to_tagged;
use crate::calls::effective_param_type;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::{CapturePolicy, StagedExpression};
use crate::expressions::values::is_plain_go_call;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::utils::observable_after_mutation;
use syntax::ast::{Expression, FormatStringPart, Literal};
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
    ) -> StagedExpression {
        if matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        ) {
            return self.stage_operand(expression, ExpressionContext::value());
        }

        let staged = self.stage_operand(expression, ExpressionContext::value());
        let mut setup = staged.setup;
        let temp_var = self.hoist_tmp_value_statement(&mut setup, prefix, &staged.value);
        StagedExpression {
            setup,
            value: temp_var,
            capture: CapturePolicy::Never,
            non_literal: false,
            has_call: false,
            has_effectful_call: false,
            call_pin_exempt: false,
        }
    }

    /// Pin a staged operand's value into a temp so it evaluates before any
    /// later sibling.
    pub(crate) fn pin_staged(&mut self, staged: &mut StagedExpression, prefix: &str) {
        let value = std::mem::take(&mut staged.value);
        let tmp = self.hoist_tmp_value_statement(&mut staged.setup, prefix, &value);
        staged.value = tmp;
        staged.capture = CapturePolicy::Never;
        staged.has_call = false;
        staged.has_effectful_call = false;
    }

    pub(crate) fn emit_force_capture(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        prefix: &str,
    ) -> String {
        if !observable_after_mutation(expression) {
            return self.capture_operand_into(setup, expression);
        }

        let (composite_setup, expression_string) = self
            .lower_composite_value(expression, ExpressionContext::value())
            .into_parts();
        setup.extend(composite_setup);
        self.hoist_tmp_value_statement(setup, prefix, &expression_string)
    }

    /// Plan an expression and capture its typed setup and value.
    pub(crate) fn stage_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> StagedExpression {
        let plan = self.plan_operand(expression, ctx);
        let mut staged = StagedExpression::from_plan(plan, expression);
        self.refine_stage_flags(&mut staged, expression);
        staged
    }

    /// Stage an expression as a composite value, capturing typed setup.
    pub(crate) fn stage_composite(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> StagedExpression {
        let plan = self.lower_composite_value(expression, ctx);
        let mut staged = StagedExpression::from_plan(plan, expression);
        self.refine_stage_flags(&mut staged, expression);
        staged
    }

    /// From typed setup + value, with the exemption refinement applied.
    pub(crate) fn staged_from_typed_setup(
        &self,
        setup: Vec<LoweredStatement>,
        value: String,
        expression: &Expression,
    ) -> StagedExpression {
        let mut staged = StagedExpression::from_typed_setup(setup, value, expression);
        self.refine_stage_flags(&mut staged, expression);
        staged
    }

    /// Exempt reads of never-mutated bindings (casts are pure conversions)
    /// and values lowering to plain Go calls, which Go orders lexically.
    /// Constructor calls are excluded: they can lower to a conversion or
    /// struct literal, whose operand reads Go does not order.
    fn refine_stage_flags(&self, staged: &mut StagedExpression, expression: &Expression) {
        staged.has_effectful_call = staged.has_call && self.contains_effectful_call(expression);
        let mut inner = expression.unwrap_parens();
        while let Expression::Cast { expression, .. } = inner {
            inner = expression.unwrap_parens();
        }
        if self.identifier_immune_to_calls(inner) {
            staged.call_pin_exempt = true;
            return;
        }
        if let Expression::Call {
            expression: callee, ..
        } = inner
            && is_plain_go_call(&staged.value)
            && !self.callee_lowers_to_type_construction(callee)
        {
            staged.call_pin_exempt = true;
        }
    }

    /// `Some`/`Ok`/`Err` lower to prelude constructor calls (their non-call
    /// nilable-slot form already fails the syntactic check).
    fn callee_lowers_to_type_construction(&self, callee: &Expression) -> bool {
        let name = match callee.unwrap_parens() {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            Expression::DotAccess { member, .. } => Some(member.as_str()),
            _ => None,
        };
        if matches!(name, Some("Some" | "Ok" | "Err" | "None")) {
            return false;
        }
        self.callee_definition(callee)
            .is_some_and(|definition| definition.is_type_definition())
    }

    /// Whether evaluating `expression` can mutate observable state.
    pub(crate) fn contains_effectful_call(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Call {
                expression: callee,
                args,
                spread,
                call_kind,
                ..
            } => {
                if self.is_pure_constructor_callee(callee) {
                    args.iter().any(|a| self.contains_effectful_call(a))
                        || (**spread)
                            .as_ref()
                            .is_some_and(|s| self.contains_effectful_call(s))
                } else if is_pure_native_method(callee, call_kind) {
                    self.contains_effectful_call(callee)
                        || args.iter().any(|a| self.contains_effectful_call(a))
                } else {
                    true
                }
            }
            Expression::Binary { left, right, .. } => {
                self.contains_effectful_call(left) || self.contains_effectful_call(right)
            }
            Expression::Unary { expression, .. }
            | Expression::DotAccess { expression, .. }
            | Expression::Cast { expression, .. }
            | Expression::Reference { expression, .. } => self.contains_effectful_call(expression),
            Expression::IndexedAccess {
                expression, index, ..
            } => self.contains_effectful_call(expression) || self.contains_effectful_call(index),
            Expression::Tuple { elements, .. } => {
                elements.iter().any(|e| self.contains_effectful_call(e))
            }
            Expression::StructCall {
                field_assignments,
                spread,
                ..
            } => {
                field_assignments
                    .iter()
                    .any(|f| self.contains_effectful_call(&f.value))
                    || spread
                        .as_expression()
                        .is_some_and(|s| self.contains_effectful_call(s))
            }
            Expression::Literal {
                literal: Literal::Slice(elements),
                ..
            } => elements.iter().any(|e| self.contains_effectful_call(e)),
            Expression::Literal {
                literal: Literal::FormatString(parts),
                ..
            } => parts.iter().any(|part| match part {
                FormatStringPart::Expression(e) => self.contains_effectful_call(e),
                FormatStringPart::Text(_) => false,
            }),
            Expression::Range { start, end, .. } => {
                start
                    .as_deref()
                    .is_some_and(|e| self.contains_effectful_call(e))
                    || end
                        .as_deref()
                        .is_some_and(|e| self.contains_effectful_call(e))
            }
            _ => false,
        }
    }

    fn is_pure_constructor_callee(&self, callee: &Expression) -> bool {
        let name = match callee.unwrap_parens() {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            Expression::DotAccess { member, .. } => Some(member.as_str()),
            _ => None,
        };
        if matches!(name, Some("Some" | "Ok" | "Err" | "None")) {
            return true;
        }
        self.callee_definition(callee)
            .is_some_and(|definition| definition.is_type_definition())
    }

    /// No binding id means a top-level definition, which is immutable.
    pub(crate) fn is_unmutated_identifier(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier {
                binding_id: Some(id),
                ..
            } => !self.facts.is_mutated(*id),
            Expression::Identifier {
                binding_id: None, ..
            } => true,
            _ => false,
        }
    }

    /// Only a binding mutated through an alias can be rebound by a call, so
    /// reads of alias-free bindings commute with sibling calls.
    pub(crate) fn identifier_immune_to_calls(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier {
                binding_id: Some(id),
                ..
            } => !self.facts.is_alias_mutated(*id),
            Expression::Identifier {
                binding_id: None, ..
            } => true,
            _ => false,
        }
    }

    pub(crate) fn stage_prelude_arg(
        &mut self,
        expression: &Expression,
        declared_param: Option<&syntax::types::Type>,
        param_ty: Option<&syntax::types::Type>,
    ) -> StagedExpression {
        let suppress = declared_param
            .is_some_and(|p| matches!(p.unwrap_forall(), syntax::types::Type::Function(_)));
        let arg_ctx = ExpressionContext::value().with_forced_tagged_go_function(suppress);
        let staged = self.stage_composite(expression, arg_ctx);

        if suppress {
            let mut setup = staged.setup;
            if let Some(tagged) =
                self.try_lower_arg_to_tagged(&mut setup, expression, &staged.value, param_ty)
            {
                return self.staged_from_typed_setup(setup, tagged, expression);
            }
            return self.staged_from_typed_setup(setup, staged.value, expression);
        }

        staged
    }

    /// Adapt a lowered-ABI Lisette callback to tagged shape when the callee
    /// (prelude generic or otherwise) declares a function param whose return
    /// classifies for direct (lowered) emission. Returns `None` when the arg
    /// already has tagged shape, is a lambda literal, or is a Go fn value.
    pub(crate) fn try_lower_arg_to_tagged(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        arg: &Expression,
        value: &str,
        param_ty: Option<&Type>,
    ) -> Option<String> {
        self.detect_lower_arg_to_tagged(arg, param_ty)?;
        Some(self.emit_lower_arg_to_tagged(setup, value, param_ty.unwrap()))
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
        let f = param_ty.as_function_type()?;
        self.classify_direct_emission(&f.return_type)?;
        Some(())
    }

    pub(crate) fn emit_lower_arg_to_tagged(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        value: &str,
        param_ty: &Type,
    ) -> String {
        let cb_var = self.hoist_tmp_value_statement(setup, "cb", value);
        let mut buffer = String::new();
        let tagged = lower_arg_to_tagged(self, &mut buffer, &cb_var, param_ty);
        if !buffer.is_empty() {
            setup.push(LoweredStatement::RawGo(buffer));
        }
        tagged
    }

    pub(crate) fn stage_native_method_args(
        &mut self,
        function: &Expression,
        args: &[Expression],
    ) -> Vec<StagedExpression> {
        let fn_ty = function.get_type();
        let formal_params: &[syntax::types::Type] =
            fn_ty.as_function_type().map_or(&[], |f| &f.params);
        let declared_params = self.callee_declared_params(function, args.len());
        args.iter()
            .enumerate()
            .map(|(i, arg)| {
                let declared = declared_params.and_then(|p| effective_param_type(i, p));
                self.stage_prelude_arg(arg, declared, formal_params.get(i))
            })
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
    ) {
        if wrap_to_any {
            self.require_stdlib();
            values[spread_index] = format!(
                "{}.SliceToAny({})",
                go_name::GO_STDLIB_PKG,
                values[spread_index]
            );
        }
        match combine {
            Some(c) if spread_index > c.fixed_count => {
                let element_go = self.go_type_string(&c.element_ty);
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
    /// carry it as structured setup. A later sibling with setup or a call
    /// forces an earlier inline-but-observable value to be captured to a
    /// `TempBind`, since Go otherwise reads it after the call runs.
    /// Returns `(setup_statements, values)`.
    pub(crate) fn sequence_structured(
        &mut self,
        mut stages: Vec<StagedExpression>,
        prefix: &str,
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let eager = self.function_state.eager_operand_capture();
        if !eager
            && stages
                .iter()
                .all(|s| s.setup.is_empty() && !s.has_effectful_call)
        {
            return (Vec::new(), stages.into_iter().map(|s| s.value).collect());
        }

        // Pinning hoists evaluation into setup, so a call left inline must
        // also pin when a later sibling pins or carries setup. A value
        // already reduced to a temp by its own setup evaluates nothing
        // inline and needs no ordering pin.
        let mut pins = vec![false; stages.len()];
        let mut later_has_setup = false;
        let mut later_effectful = false;
        let mut later_pins = false;
        for i in (0..stages.len()).rev() {
            let stage = &stages[i];
            let base_pin = matches!(stage.capture, CapturePolicy::IfLaterEffect)
                && (later_has_setup || (later_effectful && !stage.call_pin_exempt));
            let value_still_evaluates = stage.value.contains('(') || stage.value.contains("<-");
            let ordering_pin =
                stage.has_call && value_still_evaluates && (later_has_setup || later_pins);
            pins[i] = base_pin || ordering_pin;
            later_pins |= pins[i];
            later_has_setup |= !stage.setup.is_empty();
            later_effectful |= stage.has_effectful_call;
        }

        let mut setup: Vec<LoweredStatement> = Vec::new();
        let mut results = Vec::with_capacity(stages.len());
        for i in 0..stages.len() {
            let s_non_literal = stages[i].non_literal;
            let s_value = std::mem::take(&mut stages[i].value);
            let s_setup = std::mem::take(&mut stages[i].setup);

            setup.extend(s_setup);

            if pins[i] || (eager && s_non_literal) {
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
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let spread_index = spread.map(|s| {
            stages.push(self.stage_operand(s, ExpressionContext::value()));
            stages.len() - 1
        });
        let (setup, mut values) = self.sequence_structured(stages, prefix);
        if let Some(i) = spread_index {
            self.finalize_spread_stage(&mut values, i, wrap_to_any, combine);
        }
        (setup, values)
    }

    /// Sequence pre-built argument stages, routing a fn-shape-mismatched
    /// variadic spread through `try_emit_variadic_spread_adapter` (which stages
    /// the adapter as a sibling) and otherwise through the plain spread path.
    /// `adapter_params` are the callee's declared params used to detect the
    /// shape mismatch.
    pub(crate) fn sequence_args_with_spread_adapter(
        &mut self,
        mut stages: Vec<StagedExpression>,
        spread: Option<&Expression>,
        adapter_params: Option<&[Type]>,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        if let Some(spread) = spread
            && let Some(adapter_stage) =
                self.try_emit_variadic_spread_adapter(spread, adapter_params)
        {
            stages.push(adapter_stage);
            let spread_index = stages.len() - 1;
            let (setup, mut values) = self.sequence_structured(stages, "_arg");
            self.finalize_spread_stage(&mut values, spread_index, wrap_to_any, combine);
            return (setup, values);
        }
        self.sequence_with_spread_structured(stages, spread, wrap_to_any, "_arg", combine)
    }
}

fn is_pure_native_method(
    callee: &Expression,
    call_kind: &Option<syntax::program::CallKind>,
) -> bool {
    if !matches!(call_kind, Some(syntax::program::CallKind::NativeMethod(_))) {
        return false;
    }
    matches!(
        callee.unwrap_parens(),
        Expression::DotAccess { member, .. } if matches!(member.as_str(), "length" | "capacity")
    )
}
