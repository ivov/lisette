use super::*;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::abi::fallible_producer::ResultFusePlan;
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::abi::go_result_wrapping::partial_ok_nil_guard;
use crate::lowering::calls::bound_value::{Arms, BoundPair, FailureBody};
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::comma_ok::PairPlacement;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::bodies::ElseLayout;
use crate::target::ir::bodies::effect_statement;
use crate::target::names::source_name::SourceName;

#[derive(Clone, Copy)]
enum PartialOkValue {
    Nilable,
    NonNil,
}

struct GuardedValue {
    guard: NilGuard,
    polarity: Polarity,
    name: GoIdentifier,
}

impl GuardedValue {
    fn condition(&self) -> GoExpression {
        let value = GoExpression::identifier(self.name.clone());
        match self.polarity {
            Polarity::Positive => self.guard.non_nil_condition(value),
            Polarity::Negated => self.guard.nil_condition(value),
        }
    }
}

impl<'p> Planner<'p> {
    pub(in crate::lowering) fn try_lower_fused_result_match_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let Expression::Match { subject, arms, .. } = value else {
            return None;
        };
        self.try_lower_fused_lowered_match(subject, arms, &PlacePlan::Statement, Some(go_name))
    }

    pub(in crate::lowering) fn try_lower_fused_option_match_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let Expression::Match { subject, arms, .. } = value else {
            return None;
        };
        let fuse = self.queries().option_fuse_plan(subject)?;
        let arms = classify_option_arms(arms)?;
        let payload = arms.some_binding?;
        if !arms.none_body.get_type().is_never()
            || !forwards_payload(&self.facts, arms.some_body, payload)
        {
            return None;
        }

        self.scope.declare_go_name(go_name.spelling());
        let bound: BoundValue = fuse.bind(self, CommaOkValueSlot::Named(go_name.clone()));
        let failure = bound.failure_body(|| self.lower_block_as_body(arms.none_body));
        Some(bound.guard(failure, IfOrigin::Match { else_label: None }))
    }

    pub(in crate::lowering) fn try_lower_fused_partial_match_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let Expression::Match { subject, arms, .. } = value else {
            return None;
        };
        let (plan, arms) = self.queries().partial_outcome_arms(subject, arms)?;
        let ok_value = arms.value_binding?;
        if !arms.failure.expression.get_type().is_never()
            || !forwards_payload(&self.facts, &arms.ok.expression, ok_value)
        {
            return None;
        }

        let (mut statements, call) = self
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();
        self.scope.declare_go_name(go_name.spelling());
        let error_hint = arms.error_hint();
        let error = self.pair_status(
            error_hint.as_ref(),
            PairStatusKind::Error,
            PairPlacement::Statement,
        );
        let failure_body = self.lower_partial_failure_arm(arms, &error, &PlacePlan::Statement);
        statements.push(define_many(vec![go_name.clone(), error.clone()], call));
        let condition = non_nil_condition(GoExpression::identifier(error));
        statements.push(
            LoweredStatement::If(IfPlan::plain(condition, failure_body, ElseArm::None)).into(),
        );
        Some(statements)
    }

    pub(super) fn try_lower_fused_lowered_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
        destination: Option<&GoIdentifier>,
    ) -> Option<Vec<Statement>> {
        let fuse = self.queries().result_fuse_plan(subject)?;
        let (ok, err) = classify_result_arms(arms)?;
        let (ok_name, err_name) = fused_result_names(&ok, &err, fuse.shape())?;

        if let Some(name) = destination {
            if !fuses_into_destination(&self.facts, &fuse, &ok, &err) {
                return None;
            }
            self.scope.declare_go_name(name);
        } else if matches!(place, PlacePlan::Statement)
            && ok_name.is_none()
            && err_name.is_none()
            && arm_body_is_noop(&ok.arm.expression)
            && arm_body_is_noop(&err.arm.expression)
        {
            return self.lower_discarded_fused_call(&fuse, subject);
        }

        let carries_payload = fuse.carries_payload();
        let slot = self.fused_value_slot(destination, ok_name);
        let bound = self.bind_fused_result(fuse, slot, err_name);

        let unit = self.unit_value(None, None);
        let then_body = destination.is_none().then(|| {
            let ok_binding = if carries_payload {
                ArmBinding::alias_at(ok_name, bound.payload_identifier())
            } else {
                ArmBinding::copy_at(ok_name, Some(&unit))
            };
            self.lower_fused_arm(&[ok_binding], &ok.arm.expression, place)
        });

        let arm_place = if destination.is_some() {
            &PlacePlan::Statement
        } else {
            place
        };
        let else_body = self.lower_fused_err_arm(&bound, err_name, &err.arm.expression, arm_place);
        Some(fused_result_statements(
            bound.into_value(),
            then_body,
            else_body,
        ))
    }

    fn bind_fused_result(
        &mut self,
        fuse: ResultFusePlan<'_>,
        slot: CommaOkValueSlot,
        err_name: Option<FusedName<'_>>,
    ) -> BoundPair {
        let err_hint = err_name.map(|err| err.go_name());
        fuse.bind(self, slot, err_hint.as_ref())
    }

    fn fused_value_slot(
        &mut self,
        destination: Option<&GoIdentifier>,
        ok_name: Option<FusedName<'_>>,
    ) -> CommaOkValueSlot {
        match (destination, ok_name) {
            (Some(name), _) => CommaOkValueSlot::Named(name.clone()),
            (None, Some(ok)) => CommaOkValueSlot::Arm(self.arm_value_name(&ok.go_name())),
            (None, None) => CommaOkValueSlot::Unused,
        }
    }

    fn lower_discarded_fused_call(
        &mut self,
        fuse: &ResultFusePlan<'_>,
        subject: &Expression,
    ) -> Option<Vec<Statement>> {
        if fuse.is_bound() {
            return Some(Vec::new());
        }
        let site = CallSite::of(subject)?;
        let (mut statements, call) = self
            .plan_call_expression(site, None, ExpressionContext::value())
            .into_parts();
        statements.push(effect_statement(call));
        Some(statements)
    }

    pub(super) fn try_lower_nested_result_pair_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        if arms.iter().any(MatchArm::has_guard) {
            return None;
        }
        let fuse = self.queries().result_fuse_plan(subject)?;
        if !fuse.carries_payload() {
            return None;
        }
        let payload_ty = self.facts.ok_payload(&subject.get_type());
        let (ok_arms, err_arm, err_payload) = self.split_nested_result_arms(arms, &payload_ty)?;
        let err_name = err_payload.and_then(field_binding).flatten();
        let value_name = self.fresh_local("ret");
        let bound = self.bind_fused_result(fuse, CommaOkValueSlot::Arm(value_name), err_name);

        let value = GoExpression::identifier(bound.payload_identifier()?.clone());
        let then_body =
            self.lower_match_tree(&ok_arms, MatchSubject::Var(value), payload_ty, place);
        let then_body = finish_nested_pair_arm(then_body);
        let else_body = self.lower_fused_err_arm(&bound, err_name, &err_arm.expression, place);

        Some(bound.into_value().branch(
            Arms::SuccessFirst {
                success: then_body,
                failure_layout: ElseLayout::Block,
            },
            else_body,
            IfOrigin::Match { else_label: None },
        ))
    }

    fn split_nested_result_arms<'m>(
        &self,
        arms: &'m [MatchArm],
        payload_ty: &Type,
    ) -> Option<(Vec<MatchArm>, &'m MatchArm, Option<&'m Pattern>)> {
        let mut ok_arms = Vec::new();
        let mut err_arm: Option<(&MatchArm, Option<&Pattern>)> = None;
        for arm in arms {
            match (prelude_variant(&arm.pattern), &arm.pattern) {
                (Some((PreludeVariant::Ok, [payload])), _) => {
                    let mut inner_arm = arm.clone();
                    inner_arm.pattern = payload.clone();
                    ok_arms.push(inner_arm);
                }
                (Some((PreludeVariant::Err, [payload])), _)
                    if err_arm.is_none() && field_binding(payload).is_some() =>
                {
                    err_arm = Some((arm, Some(payload)));
                }
                (_, Pattern::WildCard { .. })
                    if err_arm.is_none()
                        && ok_arms.iter().any(|ok_arm| {
                            if has_binding_or_pattern(&ok_arm.pattern) {
                                return false;
                            }
                            let info = self.collect_pattern_info(&ok_arm.pattern, payload_ty);
                            info.is_irrefutable()
                        }) =>
                {
                    err_arm = Some((arm, None));
                }
                _ => return None,
            }
        }
        let (err_arm, err_payload) = err_arm?;
        if ok_arms.is_empty() {
            return None;
        }
        Some((ok_arms, err_arm, err_payload))
    }

    fn lower_fused_err_arm(
        &mut self,
        bound: &BoundPair,
        err_name: Option<FusedName<'_>>,
        body: &Expression,
        place: &PlacePlan,
    ) -> FailureBody {
        bound.error_body(|status| {
            let err_binding = ArmBinding::alias_at(err_name, Some(status));
            self.lower_fused_arm(&[err_binding], body, place)
        })
    }

    pub(super) fn try_lower_fused_partial_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let plan = self.queries().plan_call(CallSite::of(subject)?);
        if !self.queries().fusable_partial(&plan) {
            return None;
        }
        let (ok_arm, both_arm, err_arm) = classify_partial_arms(arms)?;

        let ok_name = simple_payload_binding(ok_arm)?;
        let err_name = simple_payload_binding(err_arm)?;
        let (both_value, both_err) = partial_both_bindings(both_arm)?;

        let ok_ty = self.facts.ok_payload(&subject.get_type());
        let ok_value = self.partial_ok_value(&ok_ty);

        let (mut statements, call) = self
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();
        let names =
            self.partial_value_names(ok_name.or(both_value), both_err.or(err_name), ok_value);

        let ok_body = self.lower_fused_arm(
            &[ArmBinding::alias_at(ok_name, names.value.as_ref())],
            &ok_arm.expression,
            place,
        );
        let both_body = self.lower_fused_arm(
            &[
                ArmBinding::alias_at(both_value, names.value.as_ref()),
                ArmBinding::alias_at(both_err, Some(&names.error)),
            ],
            &both_arm.expression,
            place,
        );

        let nil_check = names.value.as_ref().and_then(|v| {
            self.queries()
                .partial_ok_nil_check(&ok_ty, GoExpression::identifier(v.clone()))
        });

        let err_binding = ArmBinding::alias_at(err_name, Some(&names.error));
        let else_arm = self.partial_else_arm(nil_check, err_binding, err_arm, both_body, place);

        statements.push(names.error_checked_if(call, names.value.clone(), ok_body, else_arm));
        Some(statements)
    }

    fn partial_ok_value(&self, ok_ty: &Type) -> PartialOkValue {
        if self.facts.partial_ok_is_nilable(ok_ty) {
            PartialOkValue::Nilable
        } else {
            PartialOkValue::NonNil
        }
    }

    fn partial_value_names(
        &mut self,
        value_binding: Option<FusedName<'_>>,
        error_binding: Option<FusedName<'_>>,
        ok_value: PartialOkValue,
    ) -> PartialPairNames {
        let value = match (value_binding, ok_value) {
            (Some(value), _) => Some(self.arm_value_name(&value.go_name())),
            (None, PartialOkValue::Nilable) => Some(self.fresh_local("ret")),
            (None, PartialOkValue::NonNil) => None,
        };
        let error_hint = error_binding.map(|error| error.go_name());
        self.partial_pair_names(value, error_hint.as_ref())
    }

    fn partial_else_arm(
        &mut self,
        nil_check: Option<GoExpression>,
        err_binding: Option<ArmBinding<'_>>,
        err_arm: &MatchArm,
        both_body: LoweredBlock,
        place: &PlacePlan,
    ) -> ElseArm {
        let Some(check) = nil_check else {
            self.conversions
                .record_dropped_code(err_arm.expression.get_span());
            return ElseArm::block(both_body);
        };
        let err_body = self.lower_fused_arm(&[err_binding], &err_arm.expression, place);
        ElseArm::ElseIf(Box::new(IfPlan::plain(
            check,
            err_body,
            ElseArm::block(both_body),
        )))
    }

    fn partial_pair_names(
        &mut self,
        value: Option<GoIdentifier>,
        error_hint: Option<&SourceName>,
    ) -> PartialPairNames {
        let mut error = self.pair_status(
            error_hint,
            PairStatusKind::Error,
            PairPlacement::IfInitializer,
        );
        if value.as_deref() == Some(error.spelling()) {
            error = self.fresh_local(&error);
        }
        PartialPairNames { value, error }
    }

    fn lower_partial_failure_arm(
        &mut self,
        arms: PartialOutcomeArms<'_>,
        error: &GoIdentifier,
        place: &PlacePlan,
    ) -> LoweredBlock {
        let binding = arms.error_binding.map(|(name, ids)| ArmBinding::AliasEach {
            name,
            ids,
            go_name: error.clone(),
        });
        self.lower_fused_arm(&[binding], &arms.failure.expression, place)
    }

    pub(super) fn try_lower_fused_partial_outcome_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let (plan, arms) = self.queries().partial_outcome_arms(subject, arms)?;
        let (mut statements, call) = self
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();
        let value = arms
            .value_binding
            .map(|value| self.arm_value_name(&value.go_name()));
        let error_hint = arms.error_hint();
        let names = self.partial_pair_names(value, error_hint.as_ref());

        let ok_binding = ArmBinding::alias_at(arms.value_binding, names.value.as_ref());
        let ok_body = self.lower_fused_arm(&[ok_binding], &arms.ok.expression, place);
        let failure_body = self.lower_partial_failure_arm(arms, &names.error, place);
        if ok_body.renders_empty() && failure_body.renders_empty() {
            statements.push(effect_statement(call));
            return Some(statements);
        }

        let err = GoExpression::identifier(names.error.clone());
        let failure_first = ok_body.renders_empty()
            || (failure_body.ends_with_diverge() && ok_body.ends_with_diverge());
        let (condition, then_body, else_body) = if failure_first {
            (non_nil_condition(err), failure_body, ok_body)
        } else {
            (nil_condition(err), ok_body, failure_body)
        };
        let test = names.pair_condition(call, names.value.clone(), condition);
        statements.push(
            LoweredStatement::If(IfPlan::pair_if(
                test,
                then_body,
                ElseArm::block(else_body),
                IfOrigin::Match { else_label: None },
            ))
            .into(),
        );
        Some(statements)
    }

    pub(super) fn try_lower_fused_selective_partial_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let plan = self.queries().plan_call(CallSite::of(subject)?);
        if !matches!(plan.resolved.origin, CallableOrigin::GoInterop)
            || !self.queries().fusable_partial(&plan)
        {
            return None;
        }
        let arms = classify_selective_partial_arms(arms)?;
        let ok_ty = self.facts.ok_payload(&subject.get_type());
        let nil_guard = partial_ok_nil_guard(&self.facts, &ok_ty);
        let (mut statements, call) = self
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();

        let value_test = match (arms.selects_both, nil_guard) {
            (true, guard) => guard.map(|guard| (guard, Polarity::Positive)),
            (false, Some(guard)) => Some((guard, Polarity::Negated)),
            (false, None) => {
                statements.extend(self.lower_unselected_partial(call, &arms, place));
                return Some(statements);
            }
        };

        let (names, guarded_value) =
            self.selective_pair_names(arms.value_binding, arms.error_binding, value_test);
        let selected = self.lower_fused_arm(
            &[
                ArmBinding::alias_at(arms.value_binding, names.value.as_ref()),
                ArmBinding::alias_at(arms.error_binding, Some(&names.error)),
            ],
            &arms.selected.expression,
            place,
        );

        let fallback = self.lower_fused_arm(&[], &arms.fallback.expression, place);

        if selected.renders_empty() && fallback.renders_empty() {
            statements.push(effect_statement(call));
            return Some(statements);
        }

        let error = non_nil_condition(GoExpression::identifier(names.error.clone()));
        let condition = match guarded_value {
            Some(guarded) => {
                GoExpression::binary(error, GoBinaryOperator::And, guarded.condition())
            }
            None => error,
        };
        let test = names.pair_condition(call, names.value.clone(), condition);
        statements.push(LoweredStatement::If(selective_if(test, selected, fallback)).into());
        Some(statements)
    }

    fn lower_unselected_partial(
        &mut self,
        call: GoExpression,
        arms: &SelectivePartialArms<'_>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        self.conversions
            .record_dropped_code(arms.selected.expression.get_span());
        let fallback = self.lower_fused_arm(&[], &arms.fallback.expression, place);
        let mut statements = vec![effect_statement(call)];
        statements.extend(fallback.statements);
        statements
    }

    fn selective_pair_names(
        &mut self,
        value_binding: Option<FusedName<'_>>,
        error_binding: Option<FusedName<'_>>,
        value_test: Option<(NilGuard, Polarity)>,
    ) -> (PartialPairNames, Option<GuardedValue>) {
        let value = match value_binding {
            Some(value) => Some(self.arm_value_name(&value.go_name())),
            None => value_test.is_some().then(|| self.fresh_local("ret")),
        };
        let guarded_value = value_test
            .zip(value.clone())
            .map(|((guard, polarity), name)| GuardedValue {
                guard,
                polarity,
                name,
            });
        let error_hint = error_binding.map(|error| error.go_name());
        let names = self.partial_pair_names(value, error_hint.as_ref());
        (names, guarded_value)
    }

    pub(super) fn try_lower_fused_option_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let fuse = self.queries().option_fuse_plan(subject)?;
        let arms = classify_option_arms(arms)?;
        Some(self.lower_fused_option_arms(fuse, arms, place))
    }

    pub(in crate::lowering) fn lower_fused_option_arms(
        &mut self,
        fuse: OptionFusePlan<'_>,
        arms: OptionArms<'_>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        let slot = match arms.some_binding {
            Some(some) => CommaOkValueSlot::Arm(self.arm_value_name(&some.go_name())),
            None => CommaOkValueSlot::Unused,
        };
        let bound: BoundValue = fuse.bind(self, slot);

        let element = bound.payload();
        let some_binding = if bound.binds_value() {
            ArmBinding::alias_at(arms.some_binding, bound.payload_identifier())
        } else {
            ArmBinding::copy_at(arms.some_binding, element.as_ref())
        };
        let then_body = self.lower_fused_arm(&[some_binding], arms.some_body, place);
        let else_body = bound.failure_body(|| self.lower_fused_arm(&[], arms.none_body, place));

        let arms = if then_body.renders_empty() && !else_body.renders_empty() {
            Arms::FailureOnly
        } else {
            Arms::SuccessFirst {
                success: then_body,
                failure_layout: ElseLayout::Block,
            }
        };
        bound.branch(arms, else_body, IfOrigin::Match { else_label: None })
    }

    pub(in crate::lowering::patterns) fn lower_fused_arm<const N: usize>(
        &mut self,
        bindings: &[Option<ArmBinding<'_>>; N],
        body: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        self.with_binding_frame(|this| {
            let bound = bindings.each_ref().map(|binding| {
                binding.as_ref().map(|binding| match binding {
                    ArmBinding::Alias { name, go_name } => {
                        this.scope.bind_existing_identifier(
                            name.name,
                            name.binding.as_slice(),
                            go_name.clone(),
                        );
                        (go_name.clone(), None)
                    }
                    ArmBinding::AliasEach { name, ids, go_name } => {
                        this.scope
                            .bind_existing_identifier(*name, ids, go_name.clone());
                        (go_name.clone(), None)
                    }
                    ArmBinding::Copy { name, value } => {
                        let go_name = this.arm_value_name(&name.go_name());
                        this.scope.declare_go_name(&go_name);
                        let go_name =
                            this.scope
                                .bind_new_local(name.name, name.binding.as_slice(), go_name);
                        (go_name, Some((*value).clone()))
                    }
                })
            });

            let body_block = this.lower_block_to_place(body, place);
            let mut statements = Vec::new();
            for (go_name, value) in bound.iter().flatten() {
                if let Some(value) = value {
                    statements.push(define(go_name.clone(), value.clone()));
                }
            }
            statements.extend(body_block.statements);
            LoweredBlock { statements }
        })
    }
}

struct PartialPairNames {
    value: Option<GoIdentifier>,
    error: GoIdentifier,
}

impl PartialPairNames {
    fn pair_condition(
        &self,
        call: GoExpression,
        bound_value: Option<GoIdentifier>,
        condition: GoExpression,
    ) -> PairCondition {
        let value = bound_value.unwrap_or_else(GoIdentifier::blank);
        PairCondition::initialized(
            Some(Definition {
                names: vec![value, self.error.clone()],
                value: call,
            }),
            condition,
        )
    }

    fn error_checked_if(
        &self,
        call: GoExpression,
        bound_value: Option<GoIdentifier>,
        ok_body: LoweredBlock,
        else_arm: ElseArm,
    ) -> Statement {
        let condition = nil_condition(GoExpression::identifier(self.error.clone()));
        let test = self.pair_condition(call, bound_value, condition);
        LoweredStatement::If(IfPlan::pair_if(
            test,
            ok_body,
            else_arm,
            IfOrigin::Match { else_label: None },
        ))
        .into()
    }
}

fn classify_result_arms(arms: &[MatchArm]) -> Option<(ResultArm<'_>, ResultArm<'_>)> {
    if arms.len() != 2 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    let variant = |arm: &MatchArm| prelude_variant(&arm.pattern).map(|(variant, _)| variant);
    let second_is_catch_all = matches!(arms[1].pattern, Pattern::WildCard { .. });
    let explicit = |arm| ResultArm {
        arm,
        is_catch_all: false,
    };
    let catch_all = |arm| ResultArm {
        arm,
        is_catch_all: true,
    };
    match (variant(&arms[0])?, variant(&arms[1])) {
        (PreludeVariant::Ok, Some(PreludeVariant::Err)) => {
            Some((explicit(&arms[0]), explicit(&arms[1])))
        }
        (PreludeVariant::Err, Some(PreludeVariant::Ok)) => {
            Some((explicit(&arms[1]), explicit(&arms[0])))
        }
        (PreludeVariant::Ok, None) if second_is_catch_all => {
            Some((explicit(&arms[0]), catch_all(&arms[1])))
        }
        (PreludeVariant::Err, None) if second_is_catch_all => {
            Some((catch_all(&arms[1]), explicit(&arms[0])))
        }
        _ => None,
    }
}

fn classify_partial_arms(arms: &[MatchArm]) -> Option<(&MatchArm, &MatchArm, &MatchArm)> {
    if arms.len() != 3 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    let (mut ok, mut both, mut err) = (None, None, None);
    for arm in arms {
        let (PreludeVariant::Partial(variant), _) = prelude_variant(&arm.pattern)? else {
            return None;
        };
        let slot = match variant {
            PartialVariant::Ok => &mut ok,
            PartialVariant::Both => &mut both,
            PartialVariant::Err => &mut err,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(arm);
    }
    Some((ok?, both?, err?))
}

fn classify_selective_partial_arms(arms: &[MatchArm]) -> Option<SelectivePartialArms<'_>> {
    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return None;
    }
    if !matches!(arms[1].pattern, Pattern::WildCard { .. }) {
        return None;
    }
    let (selected, fallback) = (&arms[0], &arms[1]);
    let (PreludeVariant::Partial(variant), fields) = prelude_variant(&selected.pattern)? else {
        return None;
    };
    let (selects_both, value_binding, error_binding) = match (variant, fields) {
        (PartialVariant::Both, [value, error]) => {
            (true, field_binding(value)?, field_binding(error)?)
        }
        (PartialVariant::Err, [error]) => (false, None, field_binding(error)?),
        _ => return None,
    };

    Some(SelectivePartialArms {
        selects_both,
        selected,
        fallback,
        value_binding,
        error_binding,
    })
}

fn classify_partial_outcome_arms(arms: &[MatchArm]) -> Option<PartialOutcomeArms<'_>> {
    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return None;
    }

    fn ok_binding(arm: &MatchArm) -> Option<Option<FusedName<'_>>> {
        match prelude_variant(&arm.pattern)? {
            (PreludeVariant::Partial(PartialVariant::Ok), [value]) => field_binding(value),
            _ => None,
        }
    }
    let (ok, failure, value_binding) = match (ok_binding(&arms[0]), ok_binding(&arms[1])) {
        (Some(binding), None) => (&arms[0], &arms[1], binding),
        (None, Some(binding)) => (&arms[1], &arms[0], binding),
        _ => return None,
    };
    let error_binding = match &failure.pattern {
        Pattern::WildCard { .. } => None,
        Pattern::Or { patterns, .. } => partial_failure_error_binding(patterns)?,
        Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::EnumVariant { .. }
        | Pattern::Struct { .. }
        | Pattern::Tuple { .. }
        | Pattern::Identifier { .. }
        | Pattern::Slice { .. }
        | Pattern::AsBinding { .. } => return None,
    };
    Some(PartialOutcomeArms {
        ok,
        failure,
        value_binding,
        error_binding,
    })
}

fn partial_failure_error_binding(
    alternatives: &[Pattern],
) -> Option<Option<(&EcoString, Vec<BindingId>)>> {
    let [first, second] = alternatives else {
        return None;
    };
    let error_field = |pattern| match prelude_variant(pattern)? {
        (PreludeVariant::Partial(PartialVariant::Both), [Pattern::WildCard { .. }, error]) => {
            Some((PartialVariant::Both, field_binding(error)?))
        }
        (PreludeVariant::Partial(PartialVariant::Err), [error]) => {
            Some((PartialVariant::Err, field_binding(error)?))
        }
        _ => None,
    };

    let (first_variant, first_error) = error_field(first)?;
    let (second_variant, second_error) = error_field(second)?;
    if first_variant == second_variant {
        return None;
    }
    match (first_error, second_error) {
        (None, None) => Some(None),
        (Some(first), Some(second)) if first.name == second.name => {
            let ids = first.binding.into_iter().chain(second.binding).collect();
            Some(Some((first.name, ids)))
        }
        _ => None,
    }
}

pub(in crate::lowering) struct OptionArms<'a> {
    pub(in crate::lowering) some_binding: Option<FusedName<'a>>,
    pub(in crate::lowering) some_body: &'a Expression,
    pub(in crate::lowering) none_body: &'a Expression,
}

enum OptionArmKind<'a> {
    Some(Option<FusedName<'a>>),
    None,
    WildCard,
}

fn option_arm_kind(arm: &MatchArm) -> Option<OptionArmKind<'_>> {
    use OptionArmKind as ArmKind;
    if matches!(arm.pattern, Pattern::WildCard { .. }) {
        return Some(ArmKind::WildCard);
    }
    match prelude_variant(&arm.pattern)? {
        (PreludeVariant::Some, [field]) => Some(ArmKind::Some(field_binding(field)?)),
        (PreludeVariant::None, []) => Some(ArmKind::None),
        _ => None,
    }
}

fn classify_option_arms(arms: &[MatchArm]) -> Option<OptionArms<'_>> {
    use OptionArmKind as ArmKind;
    if arms.len() != 2 || arms.iter().any(|a| a.has_guard()) {
        return None;
    }
    match (option_arm_kind(&arms[0])?, option_arm_kind(&arms[1])?) {
        (ArmKind::Some(binding), ArmKind::None | ArmKind::WildCard) => Some(OptionArms {
            some_binding: binding,
            some_body: &arms[0].expression,
            none_body: &arms[1].expression,
        }),
        (ArmKind::None, ArmKind::Some(binding)) => Some(OptionArms {
            some_binding: binding,
            some_body: &arms[1].expression,
            none_body: &arms[0].expression,
        }),
        (ArmKind::None, ArmKind::WildCard) => Some(OptionArms {
            some_binding: None,
            some_body: &arms[1].expression,
            none_body: &arms[0].expression,
        }),
        _ => None,
    }
}

fn arm_body_is_noop(body: &Expression) -> bool {
    matches!(body, Expression::Unit { .. })
        || matches!(body, Expression::Block { items, .. } if items.is_empty())
}

fn forwards_payload(facts: &EmitFacts<'_>, body: &Expression, payload: FusedName<'_>) -> bool {
    payload.binding.is_some() && body.binding_id() == payload.binding && !facts.converts(body)
}

pub(in crate::lowering::patterns) fn ok_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    simple_variant_field(pattern, PreludeVariant::Ok)
}

pub(in crate::lowering::patterns) fn some_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    simple_variant_field(pattern, PreludeVariant::Some)
}

fn simple_variant_field(pattern: &Pattern, expected: PreludeVariant) -> Option<&Pattern> {
    let (variant, [field]) = prelude_variant(pattern)? else {
        return None;
    };
    (variant == expected && matches!(field, Pattern::Identifier { .. } | Pattern::WildCard { .. }))
        .then_some(field)
}

pub(in crate::lowering) fn field_binding(pattern: &Pattern) -> Option<Option<FusedName<'_>>> {
    match pattern {
        Pattern::Identifier {
            identifier,
            binding,
            ..
        } => Some(Some(FusedName {
            name: identifier,
            binding: *binding,
        })),
        Pattern::WildCard { .. } => Some(None),
        Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::EnumVariant { .. }
        | Pattern::Struct { .. }
        | Pattern::Tuple { .. }
        | Pattern::Slice { .. }
        | Pattern::Or { .. }
        | Pattern::AsBinding { .. } => None,
    }
}

fn simple_payload_binding(arm: &MatchArm) -> Option<Option<FusedName<'_>>> {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return None;
    };
    if fields.len() != 1 {
        return None;
    }
    field_binding(&fields[0])
}

fn partial_both_bindings(arm: &MatchArm) -> Option<(Option<FusedName<'_>>, Option<FusedName<'_>>)> {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return None;
    };
    if fields.len() != 2 {
        return None;
    }
    Some((field_binding(&fields[0])?, field_binding(&fields[1])?))
}

fn ok_arm_payload_is_omitted(arm: &MatchArm, shape: &LoweredReturnAbi) -> bool {
    let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
        return false;
    };
    match shape {
        LoweredReturnAbi::BareError => {
            fields.is_empty() || matches!(fields.as_slice(), [Pattern::Unit { .. }])
        }
        LoweredReturnAbi::Result { .. }
        | LoweredReturnAbi::Partial { .. }
        | LoweredReturnAbi::Option(_)
        | LoweredReturnAbi::Tuple { .. } => false,
    }
}

fn fuses_into_destination(
    facts: &EmitFacts<'_>,
    fuse: &ResultFusePlan<'_>,
    ok: &ResultArm,
    err: &ResultArm,
) -> bool {
    let Some(payload) = simple_payload_binding(ok.arm).flatten() else {
        return false;
    };
    fuse.carries_payload()
        && !ok.is_catch_all
        && !err.is_catch_all
        && err.arm.expression.get_type().is_never()
        && forwards_payload(facts, &ok.arm.expression, payload)
}

fn fused_result_names<'a>(
    ok: &ResultArm<'a>,
    err: &ResultArm<'a>,
    shape: &LoweredReturnAbi,
) -> Option<(Option<FusedName<'a>>, Option<FusedName<'a>>)> {
    let ok_name = if ok.is_catch_all {
        None
    } else {
        if simple_payload_binding(ok.arm).is_none() && !ok_arm_payload_is_omitted(ok.arm, shape) {
            return None;
        }
        simple_payload_binding(ok.arm).flatten()
    };

    let err_name = if err.is_catch_all {
        None
    } else {
        simple_payload_binding(err.arm)?;
        simple_payload_binding(err.arm).flatten()
    };
    Some((ok_name, err_name))
}

fn selective_if(test: PairCondition, selected: LoweredBlock, fallback: LoweredBlock) -> IfPlan {
    let else_arm = if selected.ends_with_diverge() {
        ElseArm::inline(fallback)
    } else {
        ElseArm::block(fallback)
    };
    IfPlan::pair_if(
        test,
        selected,
        else_arm,
        IfOrigin::Match { else_label: None },
    )
}

fn fused_result_statements(
    bound: BoundValue,
    then_body: Option<LoweredBlock>,
    else_body: FailureBody,
) -> Vec<Statement> {
    let arms = match then_body {
        None => Arms::FailureOnly,
        Some(then_body) if then_body.renders_empty() && !else_body.renders_empty() => {
            Arms::FailureOnly
        }
        Some(then_body) if else_body.ends_with_diverge() && then_body.ends_with_diverge() => {
            Arms::FailureFirst { success: then_body }
        }
        Some(then_body) => Arms::SuccessFirst {
            success: then_body,
            failure_layout: ElseLayout::Block,
        },
    };
    bound.branch(arms, else_body, IfOrigin::Match { else_label: None })
}

impl<'p> PlannerQueries<'_, 'p> {
    fn fusable_partial(&self, plan: &CallPlan<'_, '_>) -> bool {
        matches!(
            plan.resolved.abi.result.lowered(),
            Some(LoweredReturnAbi::Partial { .. })
        ) && self.call_result_keeps_layout(plan, plan.site.ty)
            && !self.facts.has_tuple_payload(plan.site.ty)
    }

    fn partial_outcome_arms<'s, 'a>(
        &self,
        subject: &'s Expression,
        arms: &'a [MatchArm],
    ) -> Option<(CallPlan<'p, 's>, PartialOutcomeArms<'a>)> {
        let plan = self.plan_call(CallSite::of(subject)?);
        if !self.fusable_partial(&plan) {
            return None;
        }
        Some((plan, classify_partial_outcome_arms(arms)?))
    }
}
