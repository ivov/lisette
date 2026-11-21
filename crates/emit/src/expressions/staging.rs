use crate::EmitFacts;
use crate::Planner;
use crate::abi::callable::CallableAbi;
use crate::ir::bodies::direct_first;
use crate::ir::bodies::{Definition, LoweredStatement, Statement};
use crate::ir::evaluation::Effects;
use crate::ir::go_expression::CompositeLayout;
use crate::ir::go_expression::GoExpressionNode;
use crate::ir::go_expression::Position;
use crate::ir::values::{CaptureBoundary, GoExpression, SequencedValues, Stability, ValuePlan};
use crate::names::go_name::GeneratedPackage;
use crate::passes::verify::verify_operand_order;
use crate::patterns::matching::prelude_constructor;
use crate::plan::calls::{CallableOrigin, FunctionBridge};
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use syntax::ast::{BindingId, Expression, IdentifierResolution, UnaryOperator};
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Clone)]
pub(crate) struct VariadicCombine {
    pub element_ty: Type,
    pub fixed_count: usize,
}

pub(crate) struct SpreadSequenceOptions {
    pub(crate) wrap_to_any: bool,
    pub(crate) combine: Option<VariadicCombine>,
    pub(crate) boundary: CaptureBoundary,
}

#[derive(Default)]
pub(crate) struct LaterStages {
    before: Effects,
    inline: Effects,
}

pub(crate) struct SequenceStage {
    pub(crate) plan: ValuePlan,
    pub(crate) prefix: &'static str,
    pub(crate) capture: bool,
    pub(crate) line: Option<Position>,
}

impl LaterStages {
    pub(crate) fn sequenced(setup: &[Statement], inline: Effects) -> Self {
        Self {
            before: setup_effects(setup),
            inline,
        }
    }

    pub(crate) fn can_change(&self, stability: Stability) -> bool {
        !self.can_follow(Effects::read_of(stability))
    }

    fn can_follow(&self, effects: Effects) -> bool {
        effects.can_move_across(self.before)
            && effects.without_go_order().can_move_across(self.inline)
    }

    pub(crate) fn prepend(&mut self, stage: &ValuePlan, capture: bool) -> bool {
        let effects = stage.effects();
        let pinned = capture || !self.can_follow(effects);
        self.before = self.before.union(setup_effects(stage.setup()));
        if pinned {
            self.before = self.before.union(effects);
        } else {
            self.inline = self.inline.union(effects);
        }
        pinned
    }
}

fn setup_effects(setup: &[Statement]) -> Effects {
    setup
        .iter()
        .try_fold(Effects::default(), |effects, statement| {
            match &statement.kind {
                LoweredStatement::Define(Definition { value, .. }) => {
                    let value = value.effects();
                    let reads = if value.reads() == Stability::Fixed {
                        Stability::Fixed
                    } else {
                        Stability::Observable
                    };
                    Some(effects.union(value).union(Effects::read_of(reads)))
                }
                _ => None,
            }
        })
        .unwrap_or_else(Effects::anything)
}

pub(crate) fn takes_prelude_callback(declared_param: Option<&Type>) -> bool {
    declared_param.is_some_and(|p| matches!(p.unwrap_forall(), Type::Function(_)))
}

fn prelude_arg_context<'a>(
    declared_param: Option<&Type>,
    param_ty: Option<&'a Type>,
) -> (bool, ExpressionContext<'a>) {
    let suppress = takes_prelude_callback(declared_param);
    let arg_ctx = ExpressionContext::value()
        .with_forced_tagged_go_function(suppress)
        .with_generic_result_slot(declared_param, param_ty);
    (suppress, arg_ctx)
}

impl Planner<'_> {
    pub(crate) fn stage_or_capture(
        &mut self,
        expression: &Expression,
        prefix: &'static str,
    ) -> ValuePlan {
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

    pub(crate) fn pin_staged(&mut self, staged: &mut ValuePlan, prefix: &'static str) {
        staged.pin(|setup, value| self.hoist_tmp_value_statement(setup, prefix, value));
    }

    pub(crate) fn eager_operand(
        &mut self,
        source: &Expression,
        mut value: ValuePlan,
        prefix: &'static str,
    ) -> ValuePlan {
        if !value.can_delay()
            && value.expression().constant_kind().is_none()
            && !self.plan_rests_in_stable_name(&value)
        {
            if self.contains_untyped_constant_shift(source) {
                value = value.conversion(self.go_type(&source.get_type()));
            }
            self.pin_staged(&mut value, prefix);
        }
        value
    }

    pub(crate) fn capture_value_at_boundary(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        prefix: &'static str,
        boundary: CaptureBoundary,
    ) -> GoExpression {
        let plan = self.plan_composite_value(expression, ExpressionContext::value());
        let requires_capture = boundary.delays_reads() && !plan.can_delay();
        let (value_setup, value) = plan.into_parts();
        setup.extend(value_setup);
        if requires_capture {
            GoExpression::identifier(self.hoist_tmp_value_statement(setup, prefix, value))
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
        prelude_constructor(callee).is_some() || self.callee_lowers_to_type_construction(callee)
    }

    pub(crate) fn plan_rests_in_stable_name(&self, plan: &ValuePlan) -> bool {
        if plan.rests_in_fixed_name() {
            return true;
        }
        plan.expression().as_identifier().is_some_and(|local| {
            plan.binds_local(local) && !self.scope.has_binding_for_local(local)
        })
    }

    pub(crate) fn path_read_stability(&self, path: &GoExpression) -> Stability {
        if path.effects().reads() == Stability::Observable {
            return Stability::Observable;
        }
        let mut stability = Stability::Fixed;
        path.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                stability = stability.max(match self.scope.source_binding_for_local(name) {
                    Some(id) => self.facts.binding_read_stability(id),
                    None => Stability::StableAcrossCalls,
                });
            }
        });
        stability
    }

    pub(crate) fn place_read_stability(&self, place: &Expression) -> Stability {
        match place.unwrap_parens() {
            Expression::Identifier { .. } => self.facts.identifier_read_stability(place),
            Expression::Call { .. } => Stability::StableAcrossCalls,
            Expression::DotAccess {
                expression,
                resolution,
                ..
            } if resolution.is_field_read()
                && !self.facts.reads_value_member(
                    resolution,
                    expression,
                    &expression.get_type(),
                ) =>
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
        let (suppress, arg_ctx) = prelude_arg_context(declared_param, param_ty);
        let staged = self.plan_composite_value(expression, arg_ctx);

        if suppress && let Some(adapter) = self.detect_lower_arg_to_tagged(expression, param_ty) {
            return staged.map_expression(|setup, value| {
                self.plan_function_layout_bridge(
                    setup,
                    value,
                    &adapter.source,
                    &adapter.target,
                    false,
                )
            });
        }

        self.convert_to_recorded_slot(expression, staged)
    }

    pub(crate) fn prelude_callback_stages_as_is(
        &self,
        callback: &Expression,
        declared_param: Option<&Type>,
        param_ty: Option<&Type>,
    ) -> bool {
        let (suppress, arg_ctx) = prelude_arg_context(declared_param, param_ty);
        suppress
            && self
                .never_result_bridge(&callback.get_type(), arg_ctx)
                .is_none()
            && (!self.is_go_callable(callback)
                || self.function_value_bridge(callback, arg_ctx).is_none())
            && self
                .detect_lower_arg_to_tagged(callback, param_ty)
                .is_none()
    }

    pub(crate) fn detect_lower_arg_to_tagged(
        &self,
        arg: &Expression,
        param_ty: Option<&Type>,
    ) -> Option<FunctionBridge> {
        if matches!(arg.unwrap_parens(), Expression::Lambda { .. }) {
            return None;
        }
        let callee = self.resolve_callable_value(arg)?;
        if matches!(callee.origin, CallableOrigin::GoInterop) {
            return None;
        }
        if !callee.abi.result.is_lowered() {
            return None;
        }
        self.callable_slot_bridge(
            callee.function_layout(self),
            param_ty?.unwrap_forall(),
            None,
            SlotOrigin::Lisette,
        )
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
            let element_go = self.go_type(&combine.element_ty);
            let leading = GoExpression::composite(
                Some(GoType::slice_of(&element_go)),
                values
                    .drain(combine.fixed_count..)
                    .map(|value| (None, value))
                    .collect(),
                CompositeLayout::Inline { padded: false },
            );
            spread = GoExpression::call(
                GoExpression::external_name("append".to_string()),
                vec![leading, GoExpression::spread(spread)],
            );
        }
        values.push(GoExpression::spread(spread));
    }

    pub(crate) fn sequence_values(
        &mut self,
        stages: Vec<ValuePlan>,
        boundary: CaptureBoundary,
        prefix: &'static str,
    ) -> SequencedValues {
        self.sequence_stages(
            stages
                .into_iter()
                .map(|plan| SequenceStage {
                    plan,
                    prefix,
                    capture: false,
                    line: None,
                })
                .collect(),
            boundary,
        )
    }

    pub(crate) fn sequence_stages(
        &mut self,
        stages: Vec<SequenceStage>,
        boundary: CaptureBoundary,
    ) -> SequencedValues {
        let eager = boundary.delays_reads();
        let mut later = LaterStages::default();
        let stages: Vec<_> = stages
            .into_iter()
            .rev()
            .map(|stage| {
                let pin = later.prepend(
                    &stage.plan,
                    stage.capture || (eager && !stage.plan.can_delay()),
                );
                (stage, pin)
            })
            .collect();

        let mut setup = Vec::new();
        let mut results = Vec::with_capacity(stages.len());
        let mut stability = Stability::Fixed;
        for (stage, pin) in stages.into_iter().rev() {
            let SequenceStage {
                plan,
                prefix,
                capture: _,
                line,
            } = stage;
            let start = setup.len();
            let can_delay = plan.can_delay();
            let (stage_setup, expression, stage_stability) = plan.into_parts_with_stability();
            setup.extend(stage_setup);
            if pin || (eager && !can_delay) {
                let tmp = self.hoist_tmp_value_statement(&mut setup, prefix, expression);
                results.push(GoExpression::identifier(tmp));
            } else {
                stability = stability.max(stage_stability);
                results.push(expression);
            }
            direct_first(&mut setup[start..], line);
        }
        if cfg!(debug_assertions) {
            verify_operand_order(&results).unwrap_or_else(|error| panic!("{error}"));
        }
        SequencedValues {
            setup,
            values: results,
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

impl EmitFacts<'_> {
    pub(crate) fn is_unmutated_identifier(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id),
                ..
            } => !self.is_mutated(*id),
            Expression::Identifier { .. } => true,
            _ => false,
        }
    }

    fn binding_read_stability(&self, id: BindingId) -> Stability {
        if !self.is_mutated(id) {
            Stability::Fixed
        } else if !self.is_alias_mutated(id) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        }
    }

    fn identifier_immune_to_calls(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id),
                ..
            } => !self.is_alias_mutated(*id),
            Expression::Identifier { .. } => true,
            _ => false,
        }
    }

    pub(crate) fn identifier_read_stability(&self, expression: &Expression) -> Stability {
        if self.is_unmutated_identifier(expression) {
            Stability::Fixed
        } else if self.identifier_immune_to_calls(expression) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        }
    }
}
