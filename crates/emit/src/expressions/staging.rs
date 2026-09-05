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
use std::iter;
use std::mem;
use syntax::ast::{Expression, IdentifierResolution};
use syntax::types::{FunctionParameter, Type};

/// Folds `f(leading, spread...)` into `f(append([]T{leading}, spread...)...)`: Go rejects the former.
#[derive(Clone)]
pub(crate) struct VariadicCombine {
    pub element_ty: Type,
    /// EmittedExpr-value index where variadic-feeding args begin.
    pub fixed_count: usize,
}

pub(crate) struct SpreadSequenceOptions {
    pub(crate) wrap_to_any: bool,
    pub(crate) combine: Option<VariadicCombine>,
    pub(crate) boundary: CaptureBoundary,
}

#[derive(Default)]
struct LaterStages {
    has_setup: bool,
    has_effectful_call: bool,
    has_pin: bool,
}

impl LaterStages {
    fn prepend(&mut self, stage: &ValuePlan) -> bool {
        let stage_has_setup = !stage.setup.is_empty();
        let later_can_change_value = self.has_setup
            || (self.has_effectful_call && !stage.evaluation.stability.is_stable_across_calls());
        let value_pin = !stage_has_setup
            && stage.evaluation.stability.is_observable()
            && later_can_change_value;
        let ordering_pin = stage.evaluation.effect.has_call()
            && stage.expression.contains_deferred_evaluation()
            && (self.has_setup || self.has_pin);
        let pinned = value_pin || ordering_pin;

        self.has_setup |= stage_has_setup;
        self.has_effectful_call |= stage.evaluation.effect.has_effectful_call();
        self.has_pin |= pinned;
        pinned
    }
}

impl Planner<'_> {
    pub(crate) fn stage_or_capture(&mut self, expression: &Expression, prefix: &str) -> ValuePlan {
        if matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        ) {
            return self.plan_operand(expression, ExpressionContext::value());
        }

        let staged = self.plan_operand(expression, ExpressionContext::value());
        let (mut setup, value) = staged.into_parts();
        let temp_var = self.hoist_tmp_value_statement(&mut setup, prefix, &value);
        ValuePlan::captured(setup, temp_var)
    }

    /// Pin a staged operand's value into a temp so it evaluates before any
    /// later sibling.
    pub(crate) fn pin_staged(&mut self, staged: &mut ValuePlan, prefix: &str) {
        let value =
            mem::replace(&mut staged.expression, GoExpression::opaque(String::new())).rendered();
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
        let plan = self.lower_composite_value(expression, ExpressionContext::value());
        let requires_capture = boundary.requires_value_capture(plan.evaluation.stability);
        let (value_setup, expression_string) = plan.into_parts();
        setup.extend(value_setup);
        if requires_capture {
            self.hoist_tmp_value_statement(setup, prefix, &expression_string)
        } else {
            expression_string
        }
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
                resolution: IdentifierResolution::Binding(id),
                ..
            } => !self.facts.is_mutated(*id),
            Expression::Identifier { .. } => true,
            _ => false,
        }
    }

    /// Whether readers can name the value where it sits instead of pinning it.
    pub(crate) fn plan_rests_in_stable_name(&self, plan: &ValuePlan) -> bool {
        if plan.rests_in_fixed_name() {
            return true;
        }
        let rendered = plan.rendered();
        plan.rests_in_own_temp(&rendered) && !self.scope.has_binding_for_go_name(&rendered)
    }

    /// How much of a read's surroundings can change the value it observes.
    pub(crate) fn identifier_read_stability(&self, expression: &Expression) -> Stability {
        if self.is_unmutated_identifier(expression) {
            Stability::Fixed
        } else if self.identifier_immune_to_calls(expression) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        }
    }

    /// Only a binding mutated through an alias can be rebound by a call, so
    /// reads of alias-free bindings commute with sibling calls.
    pub(crate) fn identifier_immune_to_calls(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id),
                ..
            } => !self.facts.is_alias_mutated(*id),
            Expression::Identifier { .. } => true,
            _ => false,
        }
    }

    pub(crate) fn stage_prelude_arg(
        &mut self,
        expression: &Expression,
        declared_param: Option<&Type>,
        param_ty: Option<&Type>,
    ) -> ValuePlan {
        let suppress =
            declared_param.is_some_and(|p| matches!(p.unwrap_forall(), Type::Function(_)));
        let arg_ctx = ExpressionContext::value().with_forced_tagged_go_function(suppress);
        let staged = self.lower_composite_value(expression, arg_ctx);

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

    pub(crate) fn stage_native_method_args_from(
        &mut self,
        function: &Expression,
        args: &[Expression],
        start_index: usize,
    ) -> Vec<ValuePlan> {
        let params = self.resolve_callable_params(function, args.len());
        args.iter()
            .enumerate()
            .skip(start_index)
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
                let element_go = self.use_go_type(&c.element_ty);
                let leading = values[c.fixed_count..spread_index]
                    .iter()
                    .map(GoExpression::rendered)
                    .collect::<Vec<_>>()
                    .join(", ");
                let spread_value = values[spread_index].rendered();
                let combined = format!("append([]{element_go}{{{leading}}}, {spread_value}...)...");
                values.splice(
                    c.fixed_count..=spread_index,
                    iter::once(GoExpression::opaque_with_deferred_evaluation(
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
        let mut later = LaterStages::default();
        for i in (0..stages.len()).rev() {
            pins[i] = later.prepend(&stages[i]);
        }

        let mut setup: Vec<LoweredStatement> = Vec::new();
        let mut results = Vec::with_capacity(stages.len());
        for i in 0..stages.len() {
            let s_non_literal = !stages[i].evaluation.stability.is_fixed();
            let s_expression = mem::replace(
                &mut stages[i].expression,
                GoExpression::opaque(String::new()),
            );
            let s_setup = mem::take(&mut stages[i].setup);

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
        adapter_params: Option<&[FunctionParameter]>,
        options: SpreadSequenceOptions,
    ) -> SequencedValues {
        let spread_index = spread.map(|spread| {
            let stage = self
                .try_emit_variadic_spread_adapter(spread, adapter_params)
                .unwrap_or_else(|| self.plan_operand(spread, ExpressionContext::value()));
            stages.push(stage);
            stages.len() - 1
        });
        let mut sequenced = self.sequence_values(stages, options.boundary, "arg");
        if let Some(i) = spread_index {
            self.finalize_spread_stage(
                &mut sequenced.values,
                i,
                options.wrap_to_any,
                options.combine,
            );
        }
        sequenced
    }
}
