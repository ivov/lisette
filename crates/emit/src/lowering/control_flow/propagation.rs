use crate::Planner;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::wrap_err::wrap_error;
use crate::lowering::planner::queries::PlannerQueries;

use crate::lookup::facts::inline_uses::{InlineDecision, analyze_inline_candidate_ids};
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lookup::state::bindings::InlineExpression;
use crate::lookup::state::scope::PairStatusKind;
use crate::lowering::abi::coercion_plan::CoercionPlan;
use crate::lowering::abi::fallible_producer::{ErrorNilGuard, GuardedPayload};
use crate::lowering::abi::fallible_producer::{OptionFusePlan, ResultFusePlan};
use crate::lowering::abi::go_result_wrapping::{
    nil_condition, non_nil_condition, unexpected_nil_error,
};
use crate::lowering::abi::go_returns;
use crate::lowering::calls::comma_ok::{CommaOkValueSlot, PairPlacement};
use crate::lowering::calls::go_interop::{LoweredCall, RawPair};
use crate::lowering::calls::unwrap_or::{MapLambda, map_lambda};
use crate::lowering::calls::wrap_err::ErrorUse;
use crate::lowering::patterns::matching::FusedName;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::PairCondition;
use crate::target::ir::bodies::{
    Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, assign, define,
    effect_statement,
};
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude as tags;
use crate::target::ir::prelude::Fallible;
use crate::target::ir::prelude::{PreludeVariant, prelude_constructor};
use crate::target::ir::values::{GoExpression, Stability};
use crate::target::types::return_abi::LoweredReturnAbi;
use syntax::ast::Expression;
use syntax::program::DotAccessResolution;
use syntax::program::SlotOrigin;

pub(in crate::lowering) fn plain_return(value: GoExpression) -> Statement {
    LoweredStatement::Return(vec![value]).into()
}

#[derive(Clone, Copy)]
pub(in crate::lowering) enum ForwardedTail<'a> {
    ErrorCall(CallSite<'a>),
    TaggedResult(&'a Expression),
}

pub(in crate::lowering) enum ErrorMapper<'a> {
    Lambda(MapLambda<'a>),
    Function(&'a Expression),
}

impl ErrorMapper<'_> {
    fn reads_error(&self) -> bool {
        match self {
            ErrorMapper::Lambda(lambda) => lambda.param.is_some(),
            ErrorMapper::Function(_) => true,
        }
    }
}

fn names_function(node: &GoExpressionNode) -> bool {
    match node {
        GoExpressionNode::Identifier(_) | GoExpressionNode::Qualified { .. } => true,
        GoExpressionNode::Instantiation { base, .. } => names_function(base),
        GoExpressionNode::Selector { base, .. } => {
            matches!(base.as_ref(), GoExpressionNode::Type(_)) || names_function(base)
        }
        _ => false,
    }
}

fn run_unread_error(setup: &mut Vec<Statement>, error: &GoExpression) {
    if !error.effects().can_erase() {
        setup.push(effect_statement(error.clone()));
    }
}

#[derive(Clone, Copy)]
enum PropagateTarget<'a> {
    Value,
    Discard,
    Bind(&'a GoIdentifier),
}

impl<'a> PropagateTarget<'a> {
    fn bound_name(self) -> Option<&'a GoIdentifier> {
        match self {
            PropagateTarget::Bind(name) => Some(name),
            PropagateTarget::Value | PropagateTarget::Discard => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorReuse {
    Allowed,
    Forbidden,
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_propagate(
        &mut self,
        expression: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let (statements, value) = self.plan_propagate_to(expression, PropagateTarget::Value);
        (
            statements,
            value.expect("a value target yields the ok value"),
        )
    }

    pub(in crate::lowering) fn lower_propagate_statement(
        &mut self,
        inner: &Expression,
    ) -> Vec<Statement> {
        self.plan_propagate_to(inner, PropagateTarget::Discard).0
    }

    pub(in crate::lowering) fn lower_propagate_binding(
        &mut self,
        inner: &Expression,
        name: &GoIdentifier,
    ) -> Vec<Statement> {
        self.plan_propagate_to(inner, PropagateTarget::Bind(name)).0
    }

    fn plan_propagate_to(
        &mut self,
        expression: &Expression,
        target: PropagateTarget<'_>,
    ) -> (Vec<Statement>, Option<GoExpression>) {
        let fallible = self
            .facts
            .fallible(&expression.get_type())
            .expect("plan_propagate called on non-Result/Option type");

        let mut statements = Vec::new();

        if !matches!(target, PropagateTarget::Value)
            && let Some(head) = self.try_lower_error_constructor(expression, &fallible)
        {
            statements.extend(head);
            if let Some(name) = target.bound_name() {
                self.declare_zero_for_dead_path(&mut statements, name, &fallible);
            }
            return (statements, None);
        }

        if let Some(fused) = self.try_plan_fused_propagate(expression, &fallible, target) {
            return fused;
        }
        if let Some(fused) = self.try_plan_mapped_error_propagate(expression, &fallible, target) {
            return fused;
        }
        if let Some(fused) = self.try_plan_fused_option_propagate(expression, &fallible, target) {
            return fused;
        }
        if let Some(fused) = self.try_plan_tagged_map_err_propagate(expression, &fallible, target) {
            return fused;
        }

        let (check_setup, check) = self.hoist_propagate_check_var(expression);
        statements.extend(check_setup);
        statements.push(
            self.build_propagate_failure_check(&check, &fallible, expression)
                .into(),
        );

        let ok_access = GoExpression::value_field(check, fallible.ok_field().to_string());
        let source = self
            .facts
            .value_layout(&expression.get_type(), SlotOrigin::Lisette);
        let target_layout = self
            .facts
            .value_layout(fallible.ok_ty(), SlotOrigin::Lisette);
        let coercion = CoercionPlan::change(
            &self.queries(),
            source
                .payload()
                .expect("propagation requires a payload layout"),
            &target_layout,
        );

        let (setup, ok_access) = coercion.lower(self, ok_access);
        statements.extend(setup);
        let value = self.propagated_value(&mut statements, target, None, Some(ok_access));
        (statements, value)
    }

    fn declare_zero_for_dead_path(
        &mut self,
        statements: &mut Vec<Statement>,
        var_name: &GoIdentifier,
        fallible: &Fallible,
    ) {
        let inner_ty = fallible.ok_ty();
        let zero = self.go_zero(inner_ty);
        if self.scope.is_go_name_declared(var_name.spelling()) {
            statements.push(assign(GoExpression::identifier(var_name.clone()), zero));
        } else {
            let go_ty = self.queries().go_type(inner_ty);
            statements.push(
                LoweredStatement::VarDeclaration {
                    name: var_name.clone(),
                    go_type: go_ty,
                    value: Some(zero),
                }
                .into(),
            );
            self.scope.declare_go_name(var_name.spelling());
        }
    }

    fn hoist_propagate_check_var(
        &mut self,
        expression: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let plan = self.plan_operand(expression, ExpressionContext::value());
        let requires_capture = !matches!(expression, Expression::Identifier { .. })
            || plan.effects().must_evaluate_once();
        let (mut setup, value) = plan.into_parts();
        if requires_capture {
            let check = self.hoist_temp(&mut setup, "check", value);
            (setup, GoExpression::identifier(check))
        } else {
            (setup, value)
        }
    }

    fn build_propagate_failure_check(
        &mut self,
        check: &GoExpression,
        fallible: &Fallible,
        expression: &Expression,
    ) -> LoweredStatement {
        let expression_ty = &expression.get_type();
        let return_layout = self.facts.value_layout(
            &self.scope.current_return_ctx().expect_ty(),
            SlotOrigin::Lisette,
        );
        let source_layout = self.facts.value_layout(expression_ty, SlotOrigin::Lisette);
        let (setup, values) = if self.queries().returns_tagged_type(expression)
            && source_layout
                .underlying()
                .same_representation(return_layout.underlying())
        {
            (Vec::new(), vec![check.clone()])
        } else {
            let mut setup = Vec::new();
            let err_expression = if fallible.is_result() {
                let error = GoExpression::value_field(
                    check.clone(),
                    tags::result_variant("Err").fields[0].to_string(),
                );
                let ValueLayout::Fallible { error: source, .. } = source_layout.underlying() else {
                    unreachable!("Result propagation requires an error layout")
                };
                let target = self
                    .facts
                    .value_layout(source.logical_type(), SlotOrigin::Lisette);
                let (error_setup, error) =
                    CoercionPlan::change(&self.queries(), source, &target).lower(self, error);
                setup.extend(error_setup);
                error
            } else {
                check.clone()
            };

            let (failure_setup, values) =
                self.propagate_failure_values(expression, fallible, err_expression);
            setup.extend(failure_setup);
            (setup, values)
        };

        go_returns::tag_check(fallible.failure_condition(check), setup, values)
    }

    fn propagate_failure_values(
        &mut self,
        operand: &Expression,
        fallible: &Fallible,
        err_expression: GoExpression,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        let mut setup = Vec::new();
        let error = fallible.is_result().then(|| {
            self.convert_error_to_return_context(&mut setup, operand, err_expression, fallible)
        });
        let values = self.failure_return_values(fallible, error);
        (setup, values)
    }

    fn propagate_error_status(
        &mut self,
        fallible: &Fallible,
        placement: PairPlacement,
        reuse: ErrorReuse,
    ) -> GoIdentifier {
        match fallible.err_ty() {
            Some(error_ty) if placement == PairPlacement::Statement => {
                let error_ty = &self.facts.without_go_aliases(error_ty);
                let reused = match reuse {
                    ErrorReuse::Allowed => self.scope.reusable_error(error_ty),
                    ErrorReuse::Forbidden => None,
                };
                let name = reused.unwrap_or_else(|| {
                    self.pair_status(None, PairStatusKind::Error, PairPlacement::Statement)
                });
                self.scope.declare_reusable_error(&name, error_ty);
                name
            }
            _ => self.pair_status(None, PairStatusKind::Error, placement),
        }
    }

    fn try_plan_fused_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        let lowered = self.queries().lowered_call(expression)?;
        let RawPair {
            has_value_slot,
            nil_guard,
            payload_layout_change,
        } = lowered.raw_pair()?;
        if !matches!(
            lowered.shape,
            LoweredReturnAbi::Result { .. } | LoweredReturnAbi::BareError
        ) {
            return None;
        }
        let LoweredCall { site, wraps, .. } = lowered;
        if !self.queries().returns_fallible() {
            return None;
        }

        let (mut statements, call) = self
            .plan_call_expression(site, None, ExpressionContext::value())
            .into_parts();
        let want_value = !matches!(target, PropagateTarget::Discard);
        let named = if has_value_slot && payload_layout_change.is_none() {
            self.declare_propagate_target(target)
        } else {
            None
        };
        let value_var = (has_value_slot && (want_value || nil_guard.is_some()))
            .then(|| named.cloned().unwrap_or_else(|| self.fresh_declared("ret")));
        let (message_setup, wraps) = self.prepare_wrap_messages(&wraps, ErrorUse::Read);
        let placement = if value_var.is_none() && message_setup.is_empty() {
            PairPlacement::IfInitializer
        } else {
            PairPlacement::Statement
        };
        let reuse = if message_setup.is_empty() {
            ErrorReuse::Allowed
        } else {
            ErrorReuse::Forbidden
        };
        let outcome_var = self.propagate_error_status(fallible, placement, reuse);
        let outcome = || GoExpression::identifier(outcome_var.clone());
        let binding = Definition {
            names: match &value_var {
                Some(value) => vec![value.clone(), outcome_var.clone()],
                None if has_value_slot => vec!["_".to_string().into(), outcome_var.clone()],
                None => vec![outcome_var.clone()],
            },
            value: call,
        };

        let initializer = match placement {
            PairPlacement::IfInitializer => Some(binding),
            PairPlacement::Statement => {
                statements.push(LoweredStatement::Define(binding).into());
                None
            }
        };

        statements.extend(message_setup);
        let guarded_value = || {
            GoExpression::identifier(value_var.clone().expect("nil guard requires the value var"))
        };

        let error = wrap_error(&wraps, outcome());
        let (failure_setup, failure_values) =
            self.propagate_failure_values(expression, fallible, error);
        statements.push(
            go_returns::tag_check_with_initializer(
                initializer,
                non_nil_condition(outcome()),
                failure_setup,
                failure_values,
            )
            .into(),
        );
        if let Some(guard) = nil_guard {
            let error = wrap_error(&wraps, unexpected_nil_error());
            let (nil_setup, nil_failure) =
                self.propagate_failure_values(expression, fallible, error);
            statements.push(
                go_returns::tag_check(guard.nil_condition(guarded_value()), nil_setup, nil_failure)
                    .into(),
            );
        }

        let ok_value = value_var.map(|ok_name| {
            let ok_expression = GoExpression::identifier(ok_name);
            match &payload_layout_change {
                Some(change) if want_value => {
                    self.plan_layout_change(&mut statements, ok_expression, change)
                }
                _ => ok_expression,
            }
        });

        let ok_value = ok_value.unwrap_or_else(GoExpression::unit);
        let value = self.propagated_value(&mut statements, target, named, Some(ok_value));
        Some((statements, value))
    }

    fn try_plan_fused_option_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        if fallible.is_result() || !self.queries().returns_fallible() {
            return None;
        }
        let producer = self.queries().fallible_producer(expression)?;
        let named = self.declare_propagate_target(target);
        let slot = match (named, target) {
            (Some(name), _) => CommaOkValueSlot::Named(name.clone()),
            (None, PropagateTarget::Discard) => CommaOkValueSlot::Unused,
            (None, _) => CommaOkValueSlot::Temp,
        };
        let failure_values = self.failure_return_values(fallible, None);
        let GuardedPayload {
            mut statements,
            payload,
            payload_is_index,
        } = producer.control_transfer(self, slot, failure_values);
        let payload = match (target, payload) {
            (PropagateTarget::Value, Some(payload)) if payload_is_index => Some(
                GoExpression::identifier(self.hoist_temp(&mut statements, "elem", payload)),
            ),
            (_, payload) => payload,
        };

        let value = self.propagated_value(&mut statements, target, named, payload);
        Some((statements, value))
    }

    fn try_plan_mapped_error_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        if !fallible.is_result() || !self.queries().returns_fallible() {
            return None;
        }
        let mapped = if let Some((fuse, mappers)) = self.queries().map_err_call(expression) {
            if !fuse.carries_payload() && !matches!(target, PropagateTarget::Discard) {
                return None;
            }
            Mapped::MapErr(fuse, mappers)
        } else if let Some((receiver, [error])) = self
            .queries()
            .prelude_method_call(expression, "Option", "ok_or")
        {
            Mapped::OkOr(self.queries().option_fuse_plan(receiver)?, error)
        } else if let Some((receiver, [function])) =
            self.queries()
                .prelude_method_call(expression, "Option", "ok_or_else")
        {
            let map = map_lambda(function)?;
            Mapped::OkOrElse(self.queries().option_fuse_plan(receiver)?, map)
        } else {
            return None;
        };

        let named = self.declare_propagate_target(target);
        let slot = match (named, target) {
            (Some(name), _) => CommaOkValueSlot::Named(name.clone()),
            (None, PropagateTarget::Discard) => CommaOkValueSlot::Discarded,
            (None, _) => CommaOkValueSlot::Temp,
        };

        let MappedFailure {
            mut statements,
            failure,
            payload,
            late_binding,
            mut failure_setup,
            error,
        } = self.bind_mapped_failure(mapped, slot);

        let (values_setup, failure_values) =
            self.propagate_failure_values(expression, fallible, error);
        failure_setup.extend(values_setup);
        statements.push(
            go_returns::tag_check_with_initializer(
                failure.initializer,
                failure.condition,
                failure_setup,
                failure_values,
            )
            .into(),
        );
        statements.extend(late_binding);
        let value = self.propagated_value(&mut statements, target, named, payload);
        Some((statements, value))
    }

    fn bind_mapped_failure(&mut self, mapped: Mapped<'_>, slot: CommaOkValueSlot) -> MappedFailure {
        match mapped {
            Mapped::MapErr(fuse, mappers) => {
                let nil_guard = fuse.error_nil_guard();
                let pair = fuse.bind(self, slot, None);
                let failure = pair.failure_condition();
                let status = pair.status_identifier().clone();
                let mut failure_setup = Vec::new();
                if nil_guard == ErrorNilGuard::Guarded && mappers[0].reads_error() {
                    failure_setup.push(
                        LoweredStatement::If(IfPlan::plain(
                            nil_condition(GoExpression::identifier(status.clone())),
                            LoweredBlock {
                                statements: vec![assign(
                                    GoExpression::identifier(status.clone()),
                                    unexpected_nil_error(),
                                )],
                            },
                            ElseArm::None,
                        ))
                        .into(),
                    );
                }
                let error = self.apply_error_mappers(
                    &mut failure_setup,
                    &mappers,
                    GoExpression::identifier(status),
                    Some(Stability::Fixed),
                );

                let payload = pair.payload();
                MappedFailure {
                    statements: pair.statements,
                    failure,
                    payload,
                    late_binding: None,
                    failure_setup,
                    error,
                }
            }
            Mapped::OkOr(fuse, error) => {
                let bound = fuse.bind(self, slot);
                let failure = bound.failure_condition();
                let late_binding = bound.late_binding();
                let payload = bound.payload();
                let mut statements = bound.statements;
                let value = self.plan_composite_value(error, ExpressionContext::value());
                let (error_setup, error) = self.eager_operand(value, "failure").into_parts();
                statements.extend(error_setup);
                MappedFailure {
                    statements,
                    failure,
                    payload,
                    late_binding,
                    failure_setup: Vec::new(),
                    error,
                }
            }
            Mapped::OkOrElse(fuse, map) => {
                let bound = fuse.bind(self, slot);
                let failure = bound.failure_condition();
                let late_binding = bound.late_binding();
                let payload = bound.payload();
                let (failure_setup, error) = self
                    .with_binding_frame(|this| {
                        this.plan_composite_value(map.body, ExpressionContext::value())
                    })
                    .into_parts();
                MappedFailure {
                    statements: bound.statements,
                    failure,
                    payload,
                    late_binding,
                    failure_setup,
                    error,
                }
            }
        }
    }

    fn apply_error_mappers(
        &mut self,
        setup: &mut Vec<Statement>,
        mappers: &[ErrorMapper<'_>],
        error: GoExpression,
        stability: Option<Stability>,
    ) -> GoExpression {
        let mut error = error;
        let mut stability = stability;
        for mapper in mappers {
            error = match mapper {
                ErrorMapper::Function(function) => {
                    let (function_setup, function) = self
                        .plan_composite_value(function, ExpressionContext::value())
                        .into_parts();
                    setup.extend(function_setup);
                    let function = if names_function(function.node()) {
                        function
                    } else {
                        GoExpression::identifier(self.hoist_temp(setup, "cb", function))
                    };
                    GoExpression::call(function, vec![error])
                }
                ErrorMapper::Lambda(lambda) => {
                    let (body_setup, body) = self.with_binding_frame(|this| {
                        let mut lambda_setup = Vec::new();
                        if let Some(param) = lambda.param {
                            this.bind_mapped_error(
                                &mut lambda_setup,
                                param,
                                lambda.body,
                                &error,
                                stability,
                            );
                        } else {
                            run_unread_error(&mut lambda_setup, &error);
                        }
                        let (body_setup, body) = this
                            .plan_composite_value(lambda.body, ExpressionContext::value())
                            .into_parts();
                        lambda_setup.extend(body_setup);
                        (lambda_setup, body)
                    });

                    setup.extend(body_setup);
                    body
                }
            };
            stability = None;
        }
        error
    }

    fn bind_mapped_error(
        &mut self,
        setup: &mut Vec<Statement>,
        param: FusedName<'_>,
        body: &Expression,
        error: &GoExpression,
        stability: Option<Stability>,
    ) {
        let ids = param.binding.as_slice();
        let uses = analyze_inline_candidate_ids(ids, &[body]);
        if uses == InlineDecision::Unused {
            run_unread_error(setup, error);
            return;
        }
        if let (Some(name), Some(Stability::Fixed)) = (error.as_identifier(), stability) {
            self.scope
                .bind_existing_identifier(param.name, ids, name.clone());
            return;
        }
        let read_once = uses != InlineDecision::Keep;
        if let Some(stability) = stability
            && read_once
            && stability.is_fixed()
            && error.effects().can_duplicate()
        {
            self.scope.bind_inline_expression(
                param.name,
                ids,
                InlineExpression::new(error.clone(), stability),
            );
            return;
        }
        let name = self.hoist_temp(setup, &param.go_name(), error.clone());
        self.scope.bind_existing_identifier(param.name, ids, name);
    }

    fn try_plan_tagged_map_err_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        if !fallible.is_result() {
            return None;
        }
        let (receiver, mappers) = self.queries().error_mapper_chain(expression)?;
        let receiver_ty = receiver.get_type();
        let plan = self.plan_operand(receiver, ExpressionContext::value());
        let local =
            matches!(receiver, Expression::Identifier { .. }) && plan.effects().can_duplicate();
        let operand = self.queries().plan_rests_in_stable_name(&plan);
        let (mut statements, value) = plan.into_parts();
        let (check, stability) = if operand {
            (value, Stability::Fixed)
        } else if local {
            let stability = self.queries().path_read_stability(&value);
            (value, stability)
        } else {
            let check = self.hoist_temp(&mut statements, "check", value);
            (GoExpression::identifier(check), Stability::Fixed)
        };

        let source_layout = self.facts.value_layout(&receiver_ty, SlotOrigin::Lisette);
        let ValueLayout::Fallible {
            error: error_layout,
            ..
        } = source_layout.underlying()
        else {
            return None;
        };

        let mut failure_setup = Vec::new();
        let error_target = self
            .facts
            .value_layout(error_layout.logical_type(), SlotOrigin::Lisette);
        let (error_setup, error) =
            CoercionPlan::change(&self.queries(), error_layout, &error_target).lower(
                self,
                GoExpression::value_field(
                    check.clone(),
                    tags::result_variant("Err").fields[0].to_string(),
                ),
            );

        failure_setup.extend(error_setup);
        let error = self.apply_error_mappers(&mut failure_setup, &mappers, error, Some(stability));
        let (values_setup, failure_values) =
            self.propagate_failure_values(expression, fallible, error);
        failure_setup.extend(values_setup);
        statements.push(
            go_returns::tag_check(
                fallible.failure_condition(&check),
                failure_setup,
                failure_values,
            )
            .into(),
        );

        let ok_access = GoExpression::value_field(check, fallible.ok_field().to_string());
        let target_layout = self
            .facts
            .value_layout(fallible.ok_ty(), SlotOrigin::Lisette);
        let coercion = CoercionPlan::change(
            &self.queries(),
            source_layout
                .payload()
                .expect("propagation requires a payload layout"),
            &target_layout,
        );

        let (setup, ok_access) = coercion.lower(self, ok_access);
        statements.extend(setup);
        let value = self.propagated_value(&mut statements, target, None, Some(ok_access));
        Some((statements, value))
    }

    fn declare_propagate_target<'a>(
        &mut self,
        target: PropagateTarget<'a>,
    ) -> Option<&'a GoIdentifier> {
        let name = target
            .bound_name()
            .filter(|name| !self.scope.is_go_name_declared(name.spelling()))?;
        self.scope.declare_go_name(name.spelling());
        Some(name)
    }

    fn propagated_value(
        &mut self,
        statements: &mut Vec<Statement>,
        target: PropagateTarget<'_>,
        named: Option<&GoIdentifier>,
        payload: Option<GoExpression>,
    ) -> Option<GoExpression> {
        let payload = || payload.expect("a propagated source carries its payload");
        match target {
            PropagateTarget::Value => Some(payload()),
            PropagateTarget::Discard => None,
            PropagateTarget::Bind(_) if named.is_some() => None,
            PropagateTarget::Bind(name) => {
                let ok = payload();
                statements.push(self.bind_propagate_ok(name, ok));
                None
            }
        }
    }

    fn bind_propagate_ok(&mut self, name: &GoIdentifier, ok_access: GoExpression) -> Statement {
        if self.scope.is_go_name_declared(name.spelling()) {
            assign(GoExpression::identifier(name.clone()), ok_access)
        } else {
            self.scope.declare_go_name(name.spelling());
            define(name.clone(), ok_access)
        }
    }
}

enum Mapped<'a> {
    MapErr(ResultFusePlan<'a>, Vec<ErrorMapper<'a>>),
    OkOr(OptionFusePlan<'a>, &'a Expression),
    OkOrElse(OptionFusePlan<'a>, MapLambda<'a>),
}

struct MappedFailure {
    statements: Vec<Statement>,
    failure: PairCondition,
    payload: Option<GoExpression>,
    late_binding: Option<Statement>,
    failure_setup: Vec<Statement>,
    error: GoExpression,
}

impl PlannerQueries<'_, '_> {
    fn returns_tagged_type(&self, operand: &Expression) -> bool {
        let return_ctx = self.scope.current_return_ctx();
        return_ctx.lowered_shape().is_none()
            && return_ctx.ty().is_some()
            && self.facts.checked(operand).forwards_failure()
    }

    pub(super) fn map_err_call<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<(ResultFusePlan<'a>, Vec<ErrorMapper<'a>>)> {
        let (receiver, mappers) = self.error_mapper_chain(expression)?;
        Some((self.result_fuse_plan(receiver)?, mappers))
    }

    fn error_mapper_chain<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<(&'a Expression, Vec<ErrorMapper<'a>>)> {
        let mut mappers = Vec::new();
        let mut source = expression;
        while let Some((receiver, [function])) =
            self.prelude_method_call(source, "Result", "map_err")
        {
            let Some(mapper) = self.error_mapper(function) else {
                break;
            };
            mappers.push(mapper);
            source = receiver;
        }
        if mappers.is_empty() {
            return None;
        }
        mappers.reverse();
        Some((source, mappers))
    }

    fn error_mapper<'a>(&self, function: &'a Expression) -> Option<ErrorMapper<'a>> {
        if let Some(lambda) = map_lambda(function) {
            return Some(ErrorMapper::Lambda(lambda));
        }
        let named = match function {
            Expression::Identifier { .. } => true,
            Expression::DotAccess { resolution, .. } => matches!(
                resolution,
                DotAccessResolution::PackageMember { .. }
                    | DotAccessResolution::EnumVariant { .. }
                    | DotAccessResolution::StaticMethod { .. }
                    | DotAccessResolution::InstanceMethodValue { .. }
            ),
            _ => false,
        };

        (named && !self.callee_lowers_to_type_construction(function))
            .then_some(ErrorMapper::Function(function))
    }

    pub(in crate::lowering) fn forwarded_tail<'a>(
        &self,
        check: &'a Expression,
        last: &Expression,
    ) -> Option<ForwardedTail<'a>> {
        let Expression::Propagate {
            expression: inner, ..
        } = check
        else {
            return None;
        };

        let Expression::Call {
            expression: callee,
            args,
            ..
        } = last
        else {
            return None;
        };

        if prelude_constructor(callee) != Some(PreludeVariant::Ok)
            || !matches!(args.as_slice(), [Expression::Unit { .. }])
        {
            return None;
        }
        match self.scope.current_return_ctx().lowered_shape() {
            Some(LoweredReturnAbi::BareError) => self
                .forwarded_error_call(inner)
                .map(ForwardedTail::ErrorCall),
            None => self
                .forwarded_tagged_result(inner)
                .map(ForwardedTail::TaggedResult),
            Some(_) => None,
        }
    }

    fn forwarded_error_call<'a>(&self, inner: &'a Expression) -> Option<CallSite<'a>> {
        if !self
            .facts
            .is_prelude_error(self.return_fallible()?.err_ty()?)
        {
            return None;
        }
        let lowered = self.lowered_call(inner)?;
        if lowered.shape != LoweredReturnAbi::BareError
            || !lowered.wraps.is_empty()
            || lowered.changes_layout()
            || !self
                .facts
                .is_prelude_error(&self.facts.err_payload(&lowered.site.expression.get_type()))
        {
            return None;
        }
        Some(lowered.site)
    }

    fn forwarded_tagged_result<'a>(&self, inner: &'a Expression) -> Option<&'a Expression> {
        let return_ty = self.scope.current_return_ctx().ty()?.clone();
        (self.facts.is_result(&return_ty)
            && self
                .facts
                .resolves_to_unit(&self.facts.ok_payload(&return_ty))
            && self.facts.checked(inner).forwards_failure())
        .then_some(inner)
    }

    fn returns_fallible(&self) -> bool {
        self.scope.current_return_ctx().lowered_shape().is_some()
            || self.return_fallible().is_some()
    }
}
