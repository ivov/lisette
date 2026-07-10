use crate::Planner;
use crate::abi::is_tagged_shape_fn_value;
use crate::abi::transition::lower_arg_to_tagged;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::CallableOrigin;
use crate::plan::values::{
    CaptureBoundary, EvaluationEffect, GoExpression, SequencedValues, Stability, ValuePlan,
};
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
    pub(crate) fn stage_or_capture(&mut self, expression: &Expression, prefix: &str) -> ValuePlan {
        if matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        ) {
            return self.stage_operand(expression, ExpressionContext::value());
        }

        let staged = self.stage_operand(expression, ExpressionContext::value());
        let (mut setup, value) = staged.into_parts();
        let temp_var = self.hoist_tmp_value_statement(&mut setup, prefix, &value);
        ValuePlan::captured(setup, temp_var)
    }

    /// Pin a staged operand's value into a temp so it evaluates before any
    /// later sibling.
    pub(crate) fn pin_staged(&mut self, staged: &mut ValuePlan, prefix: &str) {
        let value = std::mem::replace(&mut staged.expression, GoExpression::opaque(String::new()))
            .rendered();
        let tmp = self.hoist_tmp_value_statement(&mut staged.setup, prefix, &value);
        staged.replace_with_pinned_name(tmp);
    }

    pub(crate) fn capture_value_at_boundary(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        prefix: &str,
        boundary: CaptureBoundary,
    ) -> String {
        let loop_lifetime_reservation = if matches!(boundary, CaptureBoundary::LoopLifetime) {
            let checkpoint = self.scope.fresh_go_name_checkpoint();
            Some((checkpoint, self.fresh_var(Some(prefix))))
        } else {
            None
        };
        let plan = self.lower_composite_value(expression, ExpressionContext::value());
        let requires_capture = boundary.requires_value_capture(plan.evaluation.stability);
        let (value_setup, expression_string) = plan.into_parts();
        setup.extend(value_setup);
        if requires_capture {
            if let Some((_, reserved_name)) = loop_lifetime_reservation {
                self.declare(&reserved_name);
                setup.push(LoweredStatement::TempBind {
                    name: reserved_name.clone(),
                    value: expression_string,
                });
                reserved_name
            } else {
                self.hoist_tmp_value_statement(setup, prefix, &expression_string)
            }
        } else {
            if let Some((checkpoint, _)) = loop_lifetime_reservation {
                self.scope.restore_fresh_go_name_checkpoint(checkpoint);
            }
            expression_string
        }
    }

    /// Plan an expression and capture its typed setup and value.
    pub(crate) fn stage_operand(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        self.plan_operand(expression, ctx)
    }

    /// Stage an expression as a composite value, capturing typed setup.
    pub(crate) fn stage_composite(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        self.lower_composite_value(expression, ctx)
    }

    /// `Some`/`Ok`/`Err` lower to prelude constructor calls (their non-call
    /// nilable-slot form already fails the syntactic check).
    pub(crate) fn callee_lowers_to_type_construction(&self, callee: &Expression) -> bool {
        let name = match callee.unwrap_parens() {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            Expression::DotAccess { member, .. } => Some(member.as_str()),
            _ => None,
        };
        if matches!(name, Some("Some" | "Ok" | "Err" | "None")) {
            return false;
        }
        self.resolve_callee_definition(callee)
            .1
            .is_some_and(|definition| definition.is_type_definition())
    }

    pub(crate) fn is_pure_constructor_callee(&self, callee: &Expression) -> bool {
        let name = match callee.unwrap_parens() {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            Expression::DotAccess { member, .. } => Some(member.as_str()),
            _ => None,
        };
        if matches!(name, Some("Some" | "Ok" | "Err" | "None")) {
            return true;
        }
        self.resolve_callee_definition(callee)
            .1
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
    ) -> ValuePlan {
        let suppress = declared_param
            .is_some_and(|p| matches!(p.unwrap_forall(), syntax::types::Type::Function(_)));
        let arg_ctx = ExpressionContext::value().with_forced_tagged_go_function(suppress);
        let staged = self.stage_composite(expression, arg_ctx);

        if suppress
            && self
                .detect_lower_arg_to_tagged(expression, param_ty)
                .is_some()
        {
            return staged.map_rendered_as_computed(
                |setup, value, _contains_deferred_evaluation| {
                    let tagged = self.emit_lower_arg_to_tagged(
                        setup,
                        &value,
                        param_ty.expect("detected lowering requires a parameter type"),
                    );
                    GoExpression::opaque_with_deferred_evaluation(tagged, true)
                },
            );
        }

        staged
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
        if self
            .resolve_callable_value(arg)
            .is_some_and(|callee| matches!(callee.origin, CallableOrigin::GoInterop))
        {
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
    ) -> Vec<ValuePlan> {
        let params = self.resolve_callable_params(function, args.len());
        args.iter()
            .enumerate()
            .map(|(i, arg)| {
                let param = params.get(i).or_else(|| {
                    params
                        .last()
                        .filter(|param| param.instantiated.get_name() == Some("VarArgs"))
                });
                self.stage_prelude_arg(
                    arg,
                    param.and_then(|param| param.declared.as_ref()),
                    param.map(|param| &param.instantiated),
                )
            })
            .collect()
    }

    /// Post-staging fix-up for the spread slot: optional `any`-wrap, then
    /// either `append([]T{leading...}, spread...)...` or plain `value...`.
    pub(crate) fn finalize_spread_stage(
        &mut self,
        values: &mut Vec<GoExpression>,
        spread_index: usize,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
    ) {
        if wrap_to_any {
            self.require_stdlib();
            let rendered = format!(
                "{}.SliceToAny({})",
                go_name::GO_STDLIB_PKG,
                values[spread_index].rendered()
            );
            values[spread_index] = GoExpression::opaque_with_deferred_evaluation(rendered, true);
        }
        match combine {
            Some(c) if spread_index > c.fixed_count => {
                let element_go = self.go_type_string(&c.element_ty);
                let leading = values[c.fixed_count..spread_index]
                    .iter()
                    .map(GoExpression::rendered)
                    .collect::<Vec<_>>()
                    .join(", ");
                let spread_value = values[spread_index].rendered();
                let combined = format!("append([]{element_go}{{{leading}}}, {spread_value}...)...");
                values.splice(
                    c.fixed_count..=spread_index,
                    std::iter::once(GoExpression::opaque_with_deferred_evaluation(
                        combined, true,
                    )),
                );
            }
            _ => {
                let contains_deferred_evaluation =
                    values[spread_index].contains_deferred_evaluation();
                let rendered = format!("{}...", values[spread_index].rendered());
                values[spread_index] = GoExpression::opaque_with_deferred_evaluation(
                    rendered,
                    contains_deferred_evaluation,
                );
            }
        }
    }

    /// Sequence value plans while preserving left-to-right evaluation order.
    /// A later sibling with setup or an effectful call forces an earlier
    /// observable value into a temporary.
    pub(crate) fn sequence_values(
        &mut self,
        mut stages: Vec<ValuePlan>,
        boundary: CaptureBoundary,
        prefix: &str,
    ) -> SequencedValues {
        let effect = stages.iter().fold(EvaluationEffect::Pure, |effect, stage| {
            effect.combine(stage.evaluation.effect)
        });
        let eager = boundary.requires_value_capture(Stability::Observable);
        if !eager
            && stages.iter().all(|stage| {
                stage.setup.is_empty() && !stage.evaluation.effect.has_effectful_call()
            })
        {
            return SequencedValues {
                setup: Vec::new(),
                values: stages.into_iter().map(|stage| stage.expression).collect(),
                effect,
            };
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
            let call_pin_exempt = stage.evaluation.stability.is_stable_across_calls();
            let base_pin = stage.setup.is_empty()
                && stage.evaluation.stability.is_observable()
                && (later_has_setup || (later_effectful && !call_pin_exempt));
            let ordering_pin = stage.evaluation.effect.has_call()
                && stage.expression.contains_deferred_evaluation()
                && (later_has_setup || later_pins);
            pins[i] = base_pin || ordering_pin;
            later_pins |= pins[i];
            later_has_setup |= !stage.setup.is_empty();
            later_effectful |= stage.evaluation.effect.has_effectful_call();
        }

        let mut setup: Vec<LoweredStatement> = Vec::new();
        let mut results = Vec::with_capacity(stages.len());
        for i in 0..stages.len() {
            let s_non_literal = !stages[i].evaluation.stability.is_literal();
            let s_expression = std::mem::replace(
                &mut stages[i].expression,
                GoExpression::opaque(String::new()),
            );
            let s_setup = std::mem::take(&mut stages[i].setup);

            setup.extend(s_setup);

            if pins[i] || (eager && s_non_literal) {
                let tmp = self.fresh_var(Some(prefix));
                self.declare(&tmp);
                setup.push(LoweredStatement::TempBind {
                    name: tmp.clone(),
                    value: s_expression.rendered(),
                });
                results.push(GoExpression::name(tmp));
            } else {
                results.push(s_expression);
            }
        }
        SequencedValues {
            setup,
            values: results,
            effect,
        }
    }

    pub(crate) fn sequence_with_spread_values(
        &mut self,
        mut stages: Vec<ValuePlan>,
        spread: Option<&Expression>,
        wrap_to_any: bool,
        prefix: &str,
        combine: Option<VariadicCombine>,
        boundary: CaptureBoundary,
    ) -> SequencedValues {
        let spread_index = spread.map(|s| {
            stages.push(self.stage_operand(s, ExpressionContext::value()));
            stages.len() - 1
        });
        let mut sequenced = self.sequence_values(stages, boundary, prefix);
        if let Some(i) = spread_index {
            self.finalize_spread_stage(&mut sequenced.values, i, wrap_to_any, combine);
        }
        sequenced
    }

    pub(crate) fn sequence_args_with_spread_adapter_values(
        &mut self,
        mut stages: Vec<ValuePlan>,
        spread: Option<&Expression>,
        adapter_params: Option<&[Type]>,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
        boundary: CaptureBoundary,
    ) -> SequencedValues {
        if let Some(spread) = spread
            && let Some(adapter_stage) =
                self.try_emit_variadic_spread_adapter(spread, adapter_params)
        {
            stages.push(adapter_stage);
            let spread_index = stages.len() - 1;
            let mut sequenced = self.sequence_values(stages, boundary, "_arg");
            self.finalize_spread_stage(&mut sequenced.values, spread_index, wrap_to_any, combine);
            return sequenced;
        }
        self.sequence_with_spread_values(stages, spread, wrap_to_any, "_arg", combine, boundary)
    }
}
