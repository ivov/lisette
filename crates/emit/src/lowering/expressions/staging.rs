use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::abi::callable::CallableAbi;
use crate::lowering::calls::call_plan::{CallableOrigin, FunctionLayoutChange};
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::{SequencedValues, ValuePlan};
use crate::output::finish::verify::verify_operand_order;
use crate::target::ir::bodies::direct_first;
use crate::target::ir::bodies::{Definition, LoweredStatement, Statement};
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::CompositeLayout;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::go_expression::Position;
use crate::target::ir::prelude::prelude_constructor;
use crate::target::ir::values::{CaptureBoundary, GoExpression, Stability};
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use syntax::ast::{BindingId, Expression, IdentifierResolution, UnaryOperator};
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Clone)]
pub(in crate::lowering) struct VariadicCombine {
    pub element_ty: Type,
    pub fixed_count: usize,
}

pub(in crate::lowering) struct SpreadSequenceOptions {
    pub(in crate::lowering) wrap_to_any: bool,
    pub(in crate::lowering) combine: Option<VariadicCombine>,
    pub(in crate::lowering) boundary: CaptureBoundary,
}

#[derive(Default)]
pub(in crate::lowering) struct LaterStages {
    before: Effects,
    inline: Effects,
}

pub(in crate::lowering) struct SequenceStage {
    pub(in crate::lowering) plan: ValuePlan,
    pub(in crate::lowering) prefix: &'static str,
    pub(in crate::lowering) capture: bool,
    pub(in crate::lowering) line: Option<Position>,
}

impl LaterStages {
    pub(in crate::lowering) fn sequenced(setup: &[Statement], inline: Effects) -> Self {
        Self {
            before: setup_effects(setup),
            inline,
        }
    }

    pub(in crate::lowering) fn can_change(&self, stability: Stability) -> bool {
        !self.can_follow(Effects::read_of(stability))
    }

    fn can_follow(&self, effects: Effects) -> bool {
        effects.can_move_across(self.before)
            && effects.without_go_order().can_move_across(self.inline)
    }

    pub(in crate::lowering) fn prepend(&mut self, stage: &ValuePlan, capture: bool) -> bool {
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

pub(in crate::lowering) fn takes_prelude_callback(declared_param: Option<&Type>) -> bool {
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
    pub(in crate::lowering) fn stage_or_capture(
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
        let temp_var = self.hoist_temp(&mut setup, prefix, value);
        ValuePlan::captured(setup, temp_var)
    }

    pub(in crate::lowering) fn pin_staged(&mut self, staged: &mut ValuePlan, prefix: &'static str) {
        staged.pin(|setup, value| self.hoist_temp(setup, prefix, value));
    }

    pub(in crate::lowering) fn eager_operand(
        &mut self,
        mut value: ValuePlan,
        prefix: &'static str,
    ) -> ValuePlan {
        if !value.can_delay()
            && value.expression().constant_kind().is_none()
            && !self.plan_rests_in_stable_name(&value)
        {
            self.pin_staged(&mut value, prefix);
        }
        value
    }

    pub(in crate::lowering) fn capture_value_at_boundary(
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
            GoExpression::identifier(self.hoist_temp(setup, prefix, value))
        } else {
            value
        }
    }

    pub(in crate::lowering) fn callee_lowers_to_type_construction(
        &self,
        callee: &Expression,
    ) -> bool {
        self.resolve_callee_definition(callee)
            .1
            .is_some_and(|definition| definition.is_type_definition())
    }

    pub(in crate::lowering) fn is_pure_constructor_callee(&self, callee: &Expression) -> bool {
        prelude_constructor(callee).is_some() || self.callee_lowers_to_type_construction(callee)
    }

    pub(in crate::lowering) fn plan_rests_in_stable_name(&self, plan: &ValuePlan) -> bool {
        if plan.rests_in_fixed_name() {
            return true;
        }
        plan.expression().as_identifier().is_some_and(|local| {
            plan.binds_local(local) && !self.scope.has_binding_for_local(local)
        })
    }

    pub(in crate::lowering) fn path_read_stability(&self, path: &GoExpression) -> Stability {
        if path.effects().reads() == Stability::Observable {
            return Stability::Observable;
        }
        let mut stability = Stability::Fixed;
        path.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                stability = stability.max(match self.scope.source_binding_for_local(name) {
                    Some(id) => binding_read_stability(&self.facts, id),
                    None => Stability::StableAcrossCalls,
                });
            }
        });
        stability
    }

    pub(in crate::lowering) fn place_read_stability(&self, place: &Expression) -> Stability {
        match place {
            Expression::Identifier { .. } => identifier_read_stability(&self.facts, place),
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

    pub(in crate::lowering) fn stage_prelude_arg(
        &mut self,
        expression: &Expression,
        declared_param: Option<&Type>,
        param_ty: Option<&Type>,
    ) -> ValuePlan {
        let (suppress, arg_ctx) = prelude_arg_context(declared_param, param_ty);
        let staged = self.plan_composite_value(expression, arg_ctx);

        if suppress && let Some(adapter) = self.detect_lower_arg_to_tagged(expression, param_ty) {
            return staged.map_expression(|setup, value| {
                self.plan_function_layout_change(
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

    pub(in crate::lowering) fn prelude_callback_stages_as_is(
        &self,
        callback: &Expression,
        declared_param: Option<&Type>,
        param_ty: Option<&Type>,
    ) -> bool {
        let (suppress, arg_ctx) = prelude_arg_context(declared_param, param_ty);
        suppress
            && self
                .never_result_layout_change(&callback.get_type(), arg_ctx)
                .is_none()
            && (!self.is_go_callable(callback)
                || self
                    .function_value_layout_change(callback, arg_ctx)
                    .is_none())
            && self
                .detect_lower_arg_to_tagged(callback, param_ty)
                .is_none()
    }

    pub(in crate::lowering) fn detect_lower_arg_to_tagged(
        &self,
        arg: &Expression,
        param_ty: Option<&Type>,
    ) -> Option<FunctionLayoutChange> {
        if matches!(arg, Expression::Lambda { .. }) {
            return None;
        }
        let callee = self.resolve_callable_value(arg)?;
        if matches!(callee.origin, CallableOrigin::GoInterop) {
            return None;
        }
        if !callee.abi.result.is_lowered() {
            return None;
        }
        self.callable_slot_layout_change(
            callee.function_layout(self),
            param_ty?.unwrap_forall(),
            None,
            SlotOrigin::Lisette,
        )
    }

    pub(in crate::lowering) fn stage_native_method_args_from(
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

    pub(in crate::lowering) fn sequence_values(
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

    pub(in crate::lowering) fn sequence_stages(
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
                let tmp = self.hoist_temp(&mut setup, prefix, expression);
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

    pub(in crate::lowering) fn sequence_with_spread_values(
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

pub(in crate::lowering) fn is_unmutated_identifier(
    facts: &EmitFacts<'_>,
    expression: &Expression,
) -> bool {
    match expression {
        Expression::Identifier {
            resolution: IdentifierResolution::Binding(id),
            ..
        } => !facts.is_mutated(*id),
        Expression::Identifier { .. } => true,
        _ => false,
    }
}

fn binding_read_stability(facts: &EmitFacts<'_>, id: BindingId) -> Stability {
    if !facts.is_mutated(id) {
        Stability::Fixed
    } else if !facts.is_alias_mutated(id) {
        Stability::StableAcrossCalls
    } else {
        Stability::Observable
    }
}

fn identifier_immune_to_calls(facts: &EmitFacts<'_>, expression: &Expression) -> bool {
    match expression {
        Expression::Identifier {
            resolution: IdentifierResolution::Binding(id),
            ..
        } => !facts.is_alias_mutated(*id),
        Expression::Identifier { .. } => true,
        _ => false,
    }
}

pub(in crate::lowering) fn identifier_read_stability(
    facts: &EmitFacts<'_>,
    expression: &Expression,
) -> Stability {
    if is_unmutated_identifier(facts, expression) {
        Stability::Fixed
    } else if identifier_immune_to_calls(facts, expression) {
        Stability::StableAcrossCalls
    } else {
        Stability::Observable
    }
}
