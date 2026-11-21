use super::*;
use crate::EmitFacts;

impl Planner<'_> {
    pub(crate) fn try_lower_fused_result_match_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let Expression::Match { subject, arms, .. } = value.unwrap_parens() else {
            return None;
        };
        self.try_lower_fused_lowered_match(subject, arms, &PlacePlan::Statement, Some(go_name))
    }

    pub(crate) fn try_lower_fused_option_match_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let Expression::Match { subject, arms, .. } = value.unwrap_parens() else {
            return None;
        };
        let fuse = self.option_fuse_plan(subject)?;
        let arms = classify_option_arms(arms)?;
        let payload = arms.some_binding?;
        if !arms.none_body.get_type().is_never()
            || arm_body_is_identifier(arms.some_body) != Some(payload.name)
            || self.facts.converts(arms.some_body)
        {
            return None;
        }

        self.declare(go_name.spelling());
        let bound = fuse.bind(self, CommaOkValueSlot::Named(go_name.clone()));
        let none_condition = bound.failure_condition();
        let fail_body = self.lower_block_as_body(arms.none_body);
        let late_binding = bound.late_binding();
        let mut statements = bound.statements;
        statements
            .push(LoweredStatement::If(pair_if(none_condition, fail_body, ElseArm::None)).into());
        statements.extend(late_binding);
        Some(statements)
    }

    pub(crate) fn try_lower_fused_partial_match_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let Expression::Match { subject, arms, .. } = value.unwrap_parens() else {
            return None;
        };
        let arms = self.partial_outcome_arms(subject, arms)?;
        let ok_value = arms.value_binding?;
        if !arms.failure.expression.get_type().is_never()
            || arm_body_is_identifier(&arms.ok.expression) != Some(ok_value.name)
            || self.facts.converts(&arms.ok.expression)
        {
            return None;
        }

        let (mut statements, call) = self
            .plan_call_expression(subject, None, ExpressionContext::value())
            .into_parts();
        self.declare(go_name.spelling());
        let error_hint = arms.error_hint();
        let error = self.pair_status(error_hint.as_ref(), PairStatusKind::Error, false);
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
        let fuse = self.result_fuse_plan(subject)?;
        let (ok, err) = classify_result_arms(arms)?;

        let ok_name = if ok.is_catch_all {
            None
        } else {
            if simple_payload_binding(ok.arm).is_none()
                && !ok_arm_payload_is_omitted(ok.arm, fuse.shape())
            {
                return None;
            }
            self.facts.arm_payload_name(ok.arm)
        };
        let err_name = if err.is_catch_all {
            None
        } else {
            simple_payload_binding(err.arm)?;
            self.facts.arm_payload_name(err.arm)
        };

        if let Some(name) = destination {
            let payload = simple_payload_binding(ok.arm).flatten()?;
            if !fuse.carries_payload()
                || ok.is_catch_all
                || err.is_catch_all
                || !err.arm.expression.get_type().is_never()
                || arm_body_is_identifier(&ok.arm.expression) != Some(payload.name)
            {
                return None;
            }
            self.declare(name);
        } else if matches!(place, PlacePlan::Statement)
            && ok_name.is_none()
            && err_name.is_none()
            && arm_body_is_noop(&ok.arm.expression)
            && arm_body_is_noop(&err.arm.expression)
        {
            let (mut statements, call) = self
                .plan_call_expression(subject, None, ExpressionContext::value())
                .into_parts();
            statements.push(expression_statement(call));
            return Some(statements);
        }

        let has_nil_guard = fuse.has_nil_guard();
        let carries_payload = fuse.carries_payload();
        let slot = match (destination, ok_name) {
            (Some(name), _) => CommaOkValueSlot::Named(name.clone()),
            (None, Some(ok)) => CommaOkValueSlot::Arm(self.arm_value_name(&ok.go_name())),
            (None, None) => CommaOkValueSlot::Unused,
        };
        let err_hint = err_name.map(|err| err.go_name());
        let (mut bound, wraps) =
            fuse.bind_wrapped(self, slot, err_hint.as_ref(), err_hint.is_some());
        let error = || GoExpression::identifier(bound.status_identifier().clone());

        let unit = GoExpression::unit();
        let then_body = destination.is_none().then(|| {
            let ok_binding = if carries_payload {
                ArmBinding::alias_at(ok_name, bound.payload_identifier())
            } else {
                ArmBinding::copy_at(ok_name, Some(&unit))
            };
            let (body, uses) = self.lower_fused_arm(&[ok_binding], &ok.arm.expression, place);
            (body, uses[0])
        });
        let arm_place = if destination.is_some() {
            &PlacePlan::Statement
        } else {
            place
        };
        let err_binding = ArmBinding::alias_at(err_name, Some(bound.status_identifier()));
        let (mut else_body, err_used) =
            self.lower_fused_arm(&[err_binding], &err.arm.expression, arm_place);

        let err_read = err_used.first().copied().unwrap_or(false);
        self.prepend_error_prologue(&mut else_body, error(), has_nil_guard, err_read, &wraps);
        if matches!(then_body, Some((_, false))) {
            bound.discard_value();
        }
        let then_body = then_body.map(|(body, _)| body);
        let ok_condition = bound.success_condition();
        let err_condition = bound.failure_condition();
        let mut statements = bound.statements;

        let Some(then_body) = then_body else {
            statements.push(
                LoweredStatement::If(pair_if(err_condition, else_body, ElseArm::None)).into(),
            );
            return Some(statements);
        };

        let plan = fused_result_if(ok_condition, err_condition, then_body, else_body);
        statements.push(LoweredStatement::If(plan).into());
        Some(statements)
    }

    fn prepend_error_prologue(
        &mut self,
        else_body: &mut LoweredBlock,
        error: GoExpression,
        has_nil_guard: bool,
        err_read: bool,
        wraps: &[WrapMessage],
    ) {
        let err_read = err_read || !wraps.is_empty();
        if has_nil_guard && err_read {
            else_body.statements.insert(
                0,
                LoweredStatement::If(IfPlan::plain(
                    nil_condition(error.clone()),
                    LoweredBlock {
                        statements: vec![assign(error.clone(), unexpected_nil_error())],
                    },
                    ElseArm::None,
                ))
                .into(),
            );
        }
        if !wraps.is_empty() {
            let wrapped = self.wrap_error(wraps, error.clone());
            let prologue = vec![assign(error, wrapped)];
            let after_nil_guard = usize::from(has_nil_guard);
            else_body
                .statements
                .splice(after_nil_guard..after_nil_guard, prologue);
        }
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
        let fuse = self.result_fuse_plan(subject)?;
        if !fuse.carries_payload() {
            return None;
        }
        let payload_ty = self.facts.ok_payload(&subject.get_type());
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
                            let info = self.collect_pattern_info(&ok_arm.pattern, &payload_ty);
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
        let err_name = err_payload
            .filter(|payload| !self.facts.is_unused_binding(payload))
            .and_then(field_binding)
            .flatten();
        let err_hint = err_name.map(|err| err.go_name());

        let has_nil_guard = fuse.has_nil_guard();
        let (mut bound, wraps) = {
            let value_name = self.fresh_local(Some("ret"));
            fuse.bind_wrapped(
                self,
                CommaOkValueSlot::Arm(value_name),
                err_hint.as_ref(),
                err_hint.is_some(),
            )
        };
        let value = GoExpression::identifier(bound.payload_identifier()?.clone());
        let then_body =
            self.lower_match_tree(&ok_arms, MatchSubject::Var(value), payload_ty, place);
        let then_body = finish_nested_pair_arm(&mut bound, then_body);
        let err_binding = ArmBinding::alias_at(err_name, Some(bound.status_identifier()));
        let (mut else_body, err_used) =
            self.lower_fused_arm(&[err_binding], &err_arm.expression, place);
        let err_read = err_used.first().copied().unwrap_or(false);
        let error = GoExpression::identifier(bound.status_identifier().clone());
        self.prepend_error_prologue(&mut else_body, error, has_nil_guard, err_read, &wraps);

        let ok_condition = bound.success_condition();
        let else_arm = ElseArm::from_body(else_body, false);
        let mut statements = bound.statements;
        statements.push(LoweredStatement::If(pair_if(ok_condition, then_body, else_arm)).into());
        Some(statements)
    }

    fn fusable_partial(&self, subject: &Expression, plan: &CallPlan<'_>, selective: bool) -> bool {
        matches!(
            plan.resolved.abi.result.lowered(),
            Some(LoweredReturnAbi::Partial { .. })
        ) && (!selective || matches!(plan.resolved.origin, CallableOrigin::GoInterop))
            && self.call_result_unbridged(plan, &subject.get_type())
            && !self.facts.has_tuple_payload(&subject.get_type())
    }

    pub(super) fn try_lower_fused_partial_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let plan = self.plan_call(subject)?;
        if !self.fusable_partial(subject, &plan, false) {
            return None;
        }
        let (ok_arm, both_arm, err_arm) = classify_partial_arms(arms)?;

        let ok_name = simple_payload_binding(ok_arm)?;
        let err_name = simple_payload_binding(err_arm)?;
        let (both_val, both_err) = partial_both_bindings(both_arm)?;

        let ok_ty = self.facts.ok_payload(&subject.get_type());
        let nilable = self.facts.partial_ok_is_nilable(&ok_ty);

        let (mut statements, call) = self
            .plan_call_expression(subject, None, ExpressionContext::value())
            .into_parts();
        let val_var = match ok_name.or(both_val) {
            Some(value) => Some(self.arm_value_name(&value.go_name())),
            None => nilable.then(|| self.fresh_local(Some("ret"))),
        };
        let err_hint = both_err.or(err_name).map(|err| err.go_name());
        let mut err_var = self.pair_status(err_hint.as_ref(), PairStatusKind::Error, true);
        if val_var.as_deref() == Some(err_var.spelling()) {
            err_var = self.fresh_like(&err_var);
        }
        let err = || GoExpression::identifier(err_var.clone());

        let (ok_body, ok_uses) = self.lower_fused_arm(
            &[ArmBinding::alias_at(ok_name, val_var.as_ref())],
            &ok_arm.expression,
            place,
        );
        let (both_body, both_uses) = self.lower_fused_arm(
            &[
                ArmBinding::alias_at(both_val, val_var.as_ref()),
                ArmBinding::alias_at(both_err, Some(&err_var)),
            ],
            &both_arm.expression,
            place,
        );
        let val_used = nilable || ok_uses[0] || both_uses[0];
        let bound_value = match val_var.as_ref().filter(|_| val_used) {
            Some(v) => v.clone(),
            None => GoIdentifier::external("_".to_string()),
        };
        let initializer = Definition {
            names: vec![bound_value, err_var.clone()],
            value: call,
        };

        let nil_check = val_var
            .as_ref()
            .and_then(|v| self.partial_ok_nil_check(&ok_ty, GoExpression::identifier(v.clone())));

        let else_arm = match nil_check {
            Some(check) => {
                let (err_body, _) = self.lower_fused_arm(
                    &[ArmBinding::alias_at(err_name, Some(&err_var))],
                    &err_arm.expression,
                    place,
                );
                ElseArm::ElseIf(Box::new(IfPlan::plain(
                    check,
                    err_body,
                    ElseArm::from_body(both_body, false),
                )))
            }
            None => ElseArm::from_body(both_body, false),
        };

        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: Some(initializer),
                condition: nil_condition(err()),
                then_body: ok_body,
                else_arm,
                else_if_line: None,
                origin: IfOrigin::Match { else_label: None },
            })
            .into(),
        );
        Some(statements)
    }

    fn partial_outcome_arms<'a>(
        &mut self,
        subject: &Expression,
        arms: &'a [MatchArm],
    ) -> Option<PartialOutcomeArms<'a>> {
        let plan = self.plan_call(subject)?;
        if !self.fusable_partial(subject, &plan, false) {
            return None;
        }
        classify_partial_outcome_arms(arms)
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
            .0
    }

    pub(super) fn try_lower_fused_partial_outcome_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let arms = self.partial_outcome_arms(subject, arms)?;
        let (mut statements, call) = self
            .plan_call_expression(subject, None, ExpressionContext::value())
            .into_parts();
        let value = arms
            .value_binding
            .map(|value| self.arm_value_name(&value.go_name()));
        let error_hint = arms.error_hint();
        let mut error = self.pair_status(error_hint.as_ref(), PairStatusKind::Error, true);
        if value.as_deref() == Some(error.spelling()) {
            error = self.fresh_like(&error);
        }

        let ok_binding = ArmBinding::alias_at(arms.value_binding, value.as_ref());
        let (ok_body, [value_used]) =
            self.lower_fused_arm(&[ok_binding], &arms.ok.expression, place);
        let failure_body = self.lower_partial_failure_arm(arms, &error, place);
        if ok_body.renders_empty() && failure_body.renders_empty() {
            statements.push(expression_statement(call));
            return Some(statements);
        }

        let err = GoExpression::identifier(error.clone());
        let failure_first = ok_body.renders_empty()
            || (failure_body.ends_with_diverge() && ok_body.ends_with_diverge());
        let (condition, then_body, else_body) = if failure_first {
            (non_nil_condition(err), failure_body, ok_body)
        } else {
            (nil_condition(err), ok_body, failure_body)
        };
        let bound_value = value
            .filter(|_| value_used)
            .unwrap_or_else(|| GoIdentifier::external("_".to_string()));
        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: Some(Definition {
                    names: vec![bound_value, error],
                    value: call,
                }),
                condition,
                then_body,
                else_arm: ElseArm::from_body(else_body, false),
                else_if_line: None,
                origin: IfOrigin::Match { else_label: None },
            })
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
        let plan = self.plan_call(subject)?;
        if !self.fusable_partial(subject, &plan, true) {
            return None;
        }
        let arms = classify_selective_partial_arms(arms)?;
        let ok_ty = self.facts.ok_payload(&subject.get_type());
        let nil_guard = self.facts.partial_ok_nil_guard(&ok_ty);

        let (value_binding, error_binding) = (arms.value_binding, arms.error_binding);

        let (mut statements, call) = self
            .plan_call_expression(subject, None, ExpressionContext::value())
            .into_parts();

        if !arms.selects_both && nil_guard.is_none() {
            statements.push(expression_statement(call));
            let fallback = self
                .lower_fused_arm(&[], &arms.fallback.expression, place)
                .0;
            statements.extend(fallback.statements);
            return Some(statements);
        }

        let condition_needs_value = nil_guard.is_some();
        let value = match value_binding {
            Some(value) => Some(self.arm_value_name(&value.go_name())),
            None => condition_needs_value.then(|| self.fresh_local(Some("ret"))),
        };
        let error_hint = error_binding.map(|error| error.go_name());
        let mut error = self.pair_status(error_hint.as_ref(), PairStatusKind::Error, true);
        if value.as_deref() == Some(error.spelling()) {
            error = self.fresh_like(&error);
        }
        let (selected, binding_uses) = self.lower_fused_arm(
            &[
                ArmBinding::alias_at(value_binding, value.as_ref()),
                ArmBinding::alias_at(error_binding, Some(&error)),
            ],
            &arms.selected.expression,
            place,
        );
        let fallback = self
            .lower_fused_arm(&[], &arms.fallback.expression, place)
            .0;

        if selected.renders_empty() && fallback.renders_empty() {
            statements.push(expression_statement(call));
            return Some(statements);
        }

        let value_is_used = binding_uses[0];
        let value_slot = (condition_needs_value || value_is_used)
            .then(|| value.as_ref().expect("value use allocates a result slot"));
        let bound_value = match value_slot {
            Some(value) => value.clone(),
            None => GoIdentifier::external("_".to_string()),
        };
        let err = || GoExpression::identifier(error.clone());
        let guarded_value =
            || GoExpression::identifier(value.clone().expect("nil guard captures the value"));
        let condition = match (arms.selects_both, nil_guard) {
            (true, Some(guard)) => GoExpression::binary(
                non_nil_condition(err()),
                BinaryOp::And,
                guard.non_nil_condition(guarded_value()),
            ),
            (true, None) => non_nil_condition(err()),
            (false, guard) => {
                let guard = guard.expect("non-nilable Err returned above");
                GoExpression::binary(
                    non_nil_condition(err()),
                    BinaryOp::And,
                    guard.nil_condition(guarded_value()),
                )
            }
        };
        let selected_diverges = selected.ends_with_diverge();
        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: Some(Definition {
                    names: vec![bound_value, error.clone()],
                    value: call,
                }),
                condition,
                then_body: selected,
                else_arm: ElseArm::from_body(fallback, selected_diverges),
                else_if_line: None,
                origin: IfOrigin::Match { else_label: None },
            })
            .into(),
        );
        Some(statements)
    }

    pub(super) fn try_lower_fused_option_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let fuse = self.option_fuse_plan(subject)?;
        let arms = classify_option_arms(arms)?;
        Some(self.lower_fused_option_arms(fuse, arms, place))
    }

    pub(crate) fn lower_fused_option_arms(
        &mut self,
        fuse: OptionFusePlan<'_>,
        arms: OptionArms<'_>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        let slot = match arms.some_binding {
            Some(some) => CommaOkValueSlot::Arm(self.arm_value_name(&some.go_name())),
            None => CommaOkValueSlot::Unused,
        };
        let mut bound = fuse.bind(self, slot);

        let element = bound.payload();
        let some_binding = if bound.binds_value() {
            ArmBinding::alias_at(arms.some_binding, bound.payload_identifier())
        } else {
            ArmBinding::copy_at(arms.some_binding, element.as_ref())
        };
        let (then_body, some_uses) = self.lower_fused_arm(&[some_binding], arms.some_body, place);
        let (else_body, _) = self.lower_fused_arm(&[], arms.none_body, place);
        if !some_uses[0] {
            bound.discard_value();
        }

        let invert = then_body.renders_empty() && !else_body.renders_empty();
        let condition = if invert {
            bound.failure_condition()
        } else {
            bound.success_condition()
        };
        let plan = if invert {
            pair_if(condition, else_body, ElseArm::None)
        } else {
            pair_if(condition, then_body, ElseArm::from_body(else_body, false))
        };
        let mut statements = bound.statements;
        statements.push(LoweredStatement::If(plan).into());
        statements
    }

    pub(in crate::patterns) fn lower_fused_arm<const N: usize>(
        &mut self,
        bindings: &[Option<ArmBinding<'_>>; N],
        body: &Expression,
        place: &PlacePlan,
    ) -> (LoweredBlock, [bool; N]) {
        self.with_binding_frame(|this| {
            let bound = bindings.each_ref().map(|binding| {
                binding.as_ref().map(|binding| match binding {
                    ArmBinding::Alias { name, go_name } => {
                        this.scope.bind_identifier(
                            name.name,
                            name.binding.as_slice(),
                            go_name.clone(),
                        );
                        (go_name.clone(), None)
                    }
                    ArmBinding::AliasEach { name, ids, go_name } => {
                        this.scope.bind_identifier(*name, ids, go_name.clone());
                        (go_name.clone(), None)
                    }
                    ArmBinding::Copy { name, value } => {
                        let go_name = this.arm_value_name(&name.go_name());
                        this.declare(&go_name);
                        let go_name =
                            this.scope
                                .bind_source(name.name, name.binding.as_slice(), go_name);
                        (go_name, Some((*value).clone()))
                    }
                })
            });
            let body_block = this.lower_block_to_place(body, place);
            let used = GoUses::of(&body_block.statements);
            let mut statements = Vec::new();
            let binding_uses = bound.each_ref().map(|binding| {
                binding
                    .as_ref()
                    .is_some_and(|(go_name, _)| used.contains_identifier(go_name))
            });
            for (binding, is_used) in bound.iter().zip(&binding_uses) {
                let Some((go_name, Some(value))) = binding else {
                    continue;
                };
                if !is_used {
                    continue;
                }
                statements.push(define(go_name.clone(), value.clone()));
            }
            statements.extend(body_block.statements);
            (LoweredBlock { statements }, binding_uses)
        })
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
        _ => return None,
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

pub(crate) struct OptionArms<'a> {
    pub(crate) some_binding: Option<FusedName<'a>>,
    pub(crate) some_body: &'a Expression,
    pub(crate) none_body: &'a Expression,
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
    match body.unwrap_parens() {
        Expression::Unit { .. } => true,
        Expression::Block { items, .. } => items.is_empty(),
        _ => false,
    }
}

fn arm_body_is_identifier(body: &Expression) -> Option<&str> {
    let body = match body.unwrap_parens() {
        Expression::Block { items, .. } => match items.as_slice() {
            [single] => single.unwrap_parens(),
            _ => return None,
        },
        other => other,
    };
    match body {
        Expression::Identifier { value, .. } => Some(value.as_str()),
        _ => None,
    }
}

pub(in crate::patterns) fn ok_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    simple_variant_field(pattern, PreludeVariant::Ok)
}

pub(in crate::patterns) fn some_pattern_field(pattern: &Pattern) -> Option<&Pattern> {
    simple_variant_field(pattern, PreludeVariant::Some)
}

fn simple_variant_field(pattern: &Pattern, expected: PreludeVariant) -> Option<&Pattern> {
    let (variant, [field]) = prelude_variant(pattern)? else {
        return None;
    };
    (variant == expected && matches!(field, Pattern::Identifier { .. } | Pattern::WildCard { .. }))
        .then_some(field)
}

pub(crate) fn field_binding(pattern: &Pattern) -> Option<Option<FusedName<'_>>> {
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
        _ => None,
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

impl EmitFacts<'_> {
    fn arm_payload_name<'a>(&self, arm: &'a MatchArm) -> Option<FusedName<'a>> {
        let Pattern::EnumVariant { fields, .. } = &arm.pattern else {
            return None;
        };
        let [field] = fields.as_slice() else {
            return None;
        };
        if self.is_unused_binding(field) {
            return None;
        }
        simple_payload_binding(arm).flatten()
    }
}

fn fused_result_if(
    ok_condition: PairCondition,
    err_condition: PairCondition,
    then_body: LoweredBlock,
    else_body: LoweredBlock,
) -> IfPlan {
    if then_body.renders_empty() && !else_body.renders_empty() {
        pair_if(err_condition, else_body, ElseArm::None)
    } else if else_body.ends_with_diverge() && then_body.ends_with_diverge() {
        pair_if(
            err_condition,
            else_body,
            ElseArm::from_body(then_body, false),
        )
    } else {
        pair_if(
            ok_condition,
            then_body,
            ElseArm::from_body(else_body, false),
        )
    }
}
