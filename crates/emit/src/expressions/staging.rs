use crate::Planner;
use crate::abi::callable::CallableAbi;
use crate::abi::is_tagged_shape_fn_value;
use crate::abi::transition::lower_arg_to_tagged;
use crate::context::expression::ExpressionContext;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::matching::prelude_constructor;
use crate::plan::bodies::Statement;
use crate::plan::calls::CallableOrigin;
use crate::plan::evaluation::Effects;
use crate::plan::go_expression::CompositeLayout;
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{
    CaptureBoundary, EvaluationEffect, GoExpression, SequencedValues, Stability, ValuePlan,
};
#[cfg(debug_assertions)]
use crate::plan::verify::verify_operand_order;
use crate::utils::reads_value_member;
use syntax::ast::{BindingId, Expression, IdentifierResolution, UnaryOperator};
use syntax::types::Type;

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

/// What later operands run before an operand's expression is read.
#[derive(Default)]
pub(crate) struct LaterStages {
    before: Effects,
    inline: Effects,
}

impl LaterStages {
    pub(crate) fn sequenced(setup: &[Statement], effect: EvaluationEffect) -> Self {
        Self {
            before: if setup.is_empty() {
                Effects::default()
            } else {
                Effects::anything()
            },
            inline: Effects::calls_of(effect),
        }
    }

    pub(crate) fn can_change(&self, stability: Stability) -> bool {
        !self.can_follow(Effects::read_of(stability))
    }

    fn can_follow(&self, effects: Effects) -> bool {
        effects.can_move_across(self.before)
            && effects.without_go_order().can_move_across(self.inline)
    }

    /// Returns whether `stage` must be pinned.
    pub(crate) fn prepend(&mut self, stage: &ValuePlan) -> bool {
        let effects = stage.effects();
        let pinned = !self.can_follow(effects);
        if !stage.setup().is_empty() {
            self.before = self.before.union(Effects::anything());
        }
        if pinned {
            self.before = self.before.union(effects);
        } else {
            self.inline = self.inline.union(effects);
        }
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
        let temp_var = self.hoist_tmp_value_statement(&mut setup, prefix, value);
        ValuePlan::captured(setup, temp_var)
    }

    /// Pin a staged operand's value into a temp so it evaluates before any
    /// later sibling.
    pub(crate) fn pin_staged(&mut self, staged: &mut ValuePlan, prefix: &str) {
        staged.pin(|setup, value| self.hoist_tmp_value_statement(setup, prefix, value));
    }

    pub(crate) fn eager_operand(
        &mut self,
        source: &Expression,
        mut value: ValuePlan,
        prefix: &str,
    ) -> ValuePlan {
        if !value.can_delay()
            && value.expression().constant_kind().is_none()
            && !self.plan_rests_in_stable_name(&value)
        {
            // Keep the source type when Go would default an untyped shift to int.
            if self.contains_untyped_constant_shift(source) {
                value = value.conversion(self.use_go_type(&source.get_type()));
            }
            self.pin_staged(&mut value, prefix);
        }
        value
    }

    pub(crate) fn capture_value_at_boundary(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        prefix: &str,
        boundary: CaptureBoundary,
    ) -> GoExpression {
        let plan = self.lower_composite_value(expression, ExpressionContext::value());
        let requires_capture = boundary.delays_reads() && !plan.can_delay();
        let (value_setup, value) = plan.into_parts();
        setup.extend(value_setup);
        if requires_capture {
            GoExpression::name(self.hoist_tmp_value_statement(setup, prefix, value))
        } else {
            value
        }
    }

    pub(crate) fn callee_lowers_to_type_construction(&self, callee: &Expression) -> bool {
        self.resolve_callee_definition(callee)
            .1
            .is_some_and(|definition| definition.is_type_definition())
    }

    pub(crate) fn is_pure_constructor_callee(&self, callee: &Expression) -> bool {
        if prelude_constructor(callee).is_some() {
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

    fn binding_read_stability(&self, id: BindingId) -> Stability {
        if !self.facts.is_mutated(id) {
            Stability::Fixed
        } else if !self.facts.is_alias_mutated(id) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        }
    }

    pub(crate) fn path_read_stability(&self, path: &GoExpression) -> Stability {
        if path.effects().reads() == Stability::Observable {
            return Stability::Observable;
        }
        let mut stability = Stability::Fixed;
        path.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                stability = stability.max(
                    match self.scope.source_binding_for_go_name(name.spelling()) {
                        Some(id) => self.binding_read_stability(id),
                        None => Stability::StableAcrossCalls,
                    },
                );
            }
        });
        stability
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

    pub(crate) fn place_read_stability(&self, place: &Expression) -> Stability {
        match place.unwrap_parens() {
            Expression::Identifier { .. } => self.identifier_read_stability(place),
            Expression::Call { .. } => Stability::StableAcrossCalls,
            Expression::DotAccess {
                expression,
                resolution,
                ..
            } if resolution.is_field_read()
                && !reads_value_member(resolution, expression, &expression.get_type()) =>
            {
                Stability::Observable
            }
            Expression::DotAccess { expression, .. } => self.place_read_stability(expression),
            Expression::IndexedAccess { .. } => Stability::Observable,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => Stability::Observable,
            _ => Stability::StableAcrossCalls,
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
        let arg_ctx = ExpressionContext::value()
            .with_forced_tagged_go_function(suppress)
            .with_generic_result_slot(declared_param, param_ty);
        let staged = self.lower_composite_value(expression, arg_ctx);

        if suppress
            && self
                .detect_lower_arg_to_tagged(expression, param_ty)
                .is_some()
        {
            return staged.map_expression(|setup, value| {
                self.emit_lower_arg_to_tagged(
                    setup,
                    value,
                    param_ty.expect("detected lowering requires a parameter type"),
                )
            });
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
        setup: &mut Vec<Statement>,
        value: GoExpression,
        param_ty: &Type,
    ) -> GoExpression {
        let cb_var = self.hoist_tmp_value_statement(setup, "cb", value);
        lower_arg_to_tagged(self, setup, GoExpression::name(cb_var), param_ty)
    }

    pub(crate) fn stage_native_method_args_from(
        &mut self,
        abi: &CallableAbi,
        args: &[Expression],
        start_index: usize,
    ) -> Vec<ValuePlan> {
        args.iter()
            .enumerate()
            .skip(start_index)
            .map(|(i, arg)| {
                let param = abi.param(i);
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
    fn finalize_spread_stage(
        &mut self,
        values: &mut Vec<GoExpression>,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
    ) {
        let mut spread = values.pop().expect("a spread argument is always last");
        if wrap_to_any {
            spread = GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "SliceToAny"),
                vec![spread],
            );
        }
        if let Some(combine) = combine
            && values.len() > combine.fixed_count
        {
            let element_go = self.use_go_type(&combine.element_ty);
            let leading = GoExpression::composite(
                Some(format!("[]{element_go}")),
                values
                    .drain(combine.fixed_count..)
                    .map(|value| (None, value))
                    .collect(),
                CompositeLayout::Inline { padded: false },
            );
            spread = GoExpression::call(
                GoExpression::name("append".to_string()),
                vec![leading, GoExpression::spread(spread)],
            );
        }
        values.push(GoExpression::spread(spread));
    }

    /// Sequence value plans while preserving left-to-right evaluation order.
    pub(crate) fn sequence_values(
        &mut self,
        stages: Vec<ValuePlan>,
        boundary: CaptureBoundary,
        prefix: &str,
    ) -> SequencedValues {
        let effect = stages.iter().fold(EvaluationEffect::Pure, |effect, stage| {
            effect.combine(stage.facts().effect)
        });
        let eager = boundary.delays_reads();
        let mut later = LaterStages::default();
        let stages: Vec<_> = stages
            .into_iter()
            .rev()
            .map(|stage| {
                let pin = later.prepend(&stage);
                (stage, pin)
            })
            .collect();

        let mut setup = Vec::new();
        let mut results = Vec::with_capacity(stages.len());
        let mut stability = Stability::Fixed;
        for (stage, pin) in stages.into_iter().rev() {
            let can_delay = stage.can_delay();
            let (stage_setup, expression, evaluation) = stage.into_parts_with_facts();
            setup.extend(stage_setup);
            if pin || (eager && !can_delay) {
                let tmp = self.hoist_tmp_value_statement(&mut setup, prefix, expression);
                results.push(GoExpression::name(tmp));
            } else {
                stability = stability.max(evaluation.stability);
                results.push(expression);
            }
        }
        #[cfg(debug_assertions)]
        verify_operand_order(&results).unwrap_or_else(|error| panic!("{error}"));
        SequencedValues {
            setup,
            values: results,
            effect,
            stability,
        }
    }

    pub(crate) fn sequence_with_spread_values(
        &mut self,
        mut stages: Vec<ValuePlan>,
        spread_stage: Option<ValuePlan>,
        options: SpreadSequenceOptions,
    ) -> SequencedValues {
        let has_spread = spread_stage.is_some();
        stages.extend(spread_stage);
        let mut sequenced = self.sequence_values(stages, options.boundary, "arg");
        if has_spread {
            self.finalize_spread_stage(&mut sequenced.values, options.wrap_to_any, options.combine);
        }
        sequenced
    }
}
