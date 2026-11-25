use crate::Planner;
use crate::lookup::facts::queries::go_name_for_binding;
use crate::lowering::expressions::operand_order::OrderedOperand;
use crate::lowering::patterns::sites::TypedSubject;
use crate::lowering::planner::OuterBindings;
use crate::lowering::statements::placement::unreachable_panic_if_needed;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::GoUses;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    SelectArmKind, SelectArmPlan, SelectStatementPlan, Statement, assign, discard,
};
use crate::target::ir::go_expression::{GoExpressionNode, UnaryOp};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{PreludeVariant, prelude_variant};
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm, Span};
use syntax::program::{ChannelOperation, channel_operation};
use syntax::types::Type;

#[derive(Clone, Copy)]
enum ChannelOperationKind {
    Send,
    Receive,
}

enum PreparedChannelOperation {
    Send(GoExpression, GoExpression),
    Receive(GoExpression),
}

struct SelectReceiveContext<'a> {
    channel: &'a GoExpression,
    body: &'a Expression,
    default_body: Option<&'a Expression>,
    element_ty: Type,
    place: &'a PlacePlan<'a>,
}

enum PreparedSelectArm<'a> {
    Receive {
        binding: &'a Pattern,
        body: &'a Expression,
        channel: GoExpression,
        element_ty: Type,
    },
    Send {
        body: &'a Expression,
        operation: PreparedChannelOperation,
        span: Span,
    },
    MatchReceive {
        arms: &'a [MatchArm],
        channel: GoExpression,
        element_ty: Type,
        span: Span,
    },
    Default {
        body: &'a Expression,
        span: Span,
    },
}

impl PreparedSelectArm<'_> {
    fn span(&self) -> Span {
        match self {
            PreparedSelectArm::Receive { binding, .. } => binding.get_span(),
            PreparedSelectArm::Send { span, .. }
            | PreparedSelectArm::MatchReceive { span, .. }
            | PreparedSelectArm::Default { span, .. } => *span,
        }
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_select(
        &mut self,
        arms: &[SelectArm],
        span: &Span,
        place: &PlacePlan,
    ) -> LoweredStatement {
        let line = self.facts.source_position(span);
        let needs_retry_loop = arms.iter().any(
            |arm| matches!(arm, SelectArm::Receive { binding, .. } if is_some_pattern(binding)),
        );

        let mut setup: Vec<Statement> = Vec::new();
        let prep = self.preprocess_select_arms(&mut setup, arms, needs_retry_loop);

        let has_default = prep
            .iter()
            .any(|arm| matches!(arm, PreparedSelectArm::Default { .. }));

        let arm_plans = self.with_scope(|this| this.lower_select_arms(prep, place));

        let all_arms_diverge =
            !arm_plans.is_empty() && arm_plans.iter().all(|arm| arm.body().ends_with_diverge());
        let exhaustive = all_arms_diverge || if needs_retry_loop { false } else { has_default };
        let mut postlude: Vec<Statement> = Vec::new();
        if let Some(panic) = unreachable_panic_if_needed(place, exhaustive) {
            postlude.push(panic);
        }

        let select = LoweredStatement::Select(SelectStatementPlan { arms: arm_plans });
        if needs_retry_loop {
            let mut body = vec![select.into()];
            if !all_arms_diverge {
                body.push(LoweredStatement::Break(LoopTransfer::Unlabeled).into());
            }
            let loop_plan = LoopPlan {
                kind: LoopKind::Generated { label: None },
                header: LoopHeader::Infinite,
                body: LoweredBlock { statements: body },
            };
            let mut statements = setup;
            statements.push(Statement {
                line,
                kind: LoweredStatement::Loop(loop_plan),
            });
            statements.extend(postlude);
            LoweredStatement::Body(LoweredBlock { statements })
        } else {
            let mut statements = setup;
            statements.push(Statement { line, kind: select });
            statements.extend(postlude);
            LoweredStatement::Body(LoweredBlock { statements })
        }
    }

    fn lower_select_arms<'a>(
        &mut self,
        arms: Vec<PreparedSelectArm<'a>>,
        place: &PlacePlan,
    ) -> Vec<SelectArmPlan> {
        let default_body = arms.iter().find_map(|arm| match arm {
            PreparedSelectArm::Default { body, .. } => Some(*body),
            _ => None,
        });

        let mut arm_plans = Vec::with_capacity(arms.len());
        for arm in arms {
            let line = self.facts.source_position(&arm.span());
            let kind = match arm {
                PreparedSelectArm::Receive {
                    binding,
                    body,
                    channel,
                    element_ty,
                } => {
                    let receiver_ctx = SelectReceiveContext {
                        channel: &channel,
                        body,
                        default_body,
                        element_ty,
                        place,
                    };

                    self.lower_receive_arm(binding, &receiver_ctx)
                }
                PreparedSelectArm::Send {
                    body, operation, ..
                } => self.lower_send_arm(&operation, body, place),
                PreparedSelectArm::MatchReceive {
                    arms,
                    channel,
                    element_ty,
                    ..
                } => self.lower_match_receive_arm(arms, &channel, &element_ty, place),
                PreparedSelectArm::Default { body, .. } => SelectArmKind::Default {
                    body: self.lower_block_to_place(body, place),
                },
            };

            arm_plans.push(SelectArmPlan { kind, line });
        }
        arm_plans
    }

    fn preprocess_select_arms<'a>(
        &mut self,
        setup: &mut Vec<Statement>,
        arms: &'a [SelectArm],
        needs_retry_loop: bool,
    ) -> Vec<PreparedSelectArm<'a>> {
        let mut operands = Vec::new();
        let operations: Vec<_> = arms
            .iter()
            .map(|arm| self.plan_select_operation(arm, &mut operands))
            .collect();
        let boundary = if needs_retry_loop {
            CaptureBoundary::Delayed
        } else {
            CaptureBoundary::SiblingSequence
        };
        let sequenced = self.sequence_operands(operands, boundary);
        setup.extend(sequenced.setup);
        let mut values = sequenced.values.into_iter();
        arms.iter()
            .zip(operations)
            .map(|(arm, operation)| match arm {
                SelectArm::Send {
                    send_expression,
                    body,
                } => {
                    let channel = values.next().expect("one channel per select operation");
                    let operation = if matches!(operation, Some(ChannelOperationKind::Send)) {
                        PreparedChannelOperation::Send(channel, values.next().expect("send value"))
                    } else {
                        PreparedChannelOperation::Receive(channel)
                    };

                    PreparedSelectArm::Send {
                        body,
                        operation,
                        span: send_expression.get_span(),
                    }
                }
                SelectArm::Receive {
                    receive_expression,
                    binding,
                    body,
                } => {
                    let channel = values.next().expect("receive channel");
                    PreparedSelectArm::Receive {
                        binding,
                        body,
                        channel,
                        element_ty: receive_expression.get_type().ok_type(),
                    }
                }
                SelectArm::MatchReceive {
                    receive_expression,
                    arms,
                } => PreparedSelectArm::MatchReceive {
                    arms,
                    channel: values.next().expect("match receive channel"),
                    element_ty: receive_expression.get_type().ok_type(),
                    span: receive_expression.get_span(),
                },
                SelectArm::WildCard { span, body } => {
                    PreparedSelectArm::Default { body, span: *span }
                }
            })
            .collect()
    }

    fn plan_select_operation(
        &mut self,
        arm: &SelectArm,
        operands: &mut Vec<OrderedOperand>,
    ) -> Option<ChannelOperationKind> {
        let (plan, span, capture, send_value) = match arm {
            SelectArm::Send {
                send_expression, ..
            } => {
                let operation = channel_operation(send_expression);
                let plan = if let Some(operation) = &operation {
                    let channel = operation.channel();
                    let plan = self.plan_value(channel, ExpressionContext::value());
                    if self.facts.is_reference(&channel.get_type()) {
                        plan.map_expression(|_, value| cancel_deref_of_address(value))
                    } else {
                        plan
                    }
                } else {
                    self.plan_channel_operand(send_expression)
                };

                let send_value = match operation {
                    Some(ChannelOperation::Send { value, .. }) => Some(value),
                    _ => None,
                };
                (plan, send_expression.get_span(), false, send_value)
            }
            SelectArm::Receive {
                receive_expression, ..
            }
            | SelectArm::MatchReceive {
                receive_expression, ..
            } => {
                let capture = match arm {
                    SelectArm::Receive { binding, .. } => is_some_pattern(binding),
                    _ => false,
                };
                (
                    self.plan_channel_operand(receive_expression),
                    receive_expression.get_span(),
                    capture,
                    None,
                )
            }
            SelectArm::WildCard { .. } => return None,
        };

        let line = self.facts.source_position(&span);
        operands.push(OrderedOperand {
            plan,
            prefix: "ch",
            capture,
            line: line.clone(),
        });
        Some(if let Some(value) = send_value {
            operands.push(OrderedOperand {
                plan: self.plan_composite_value(value, ExpressionContext::value()),
                prefix: "sendVal",
                capture: false,
                line,
            });
            ChannelOperationKind::Send
        } else {
            ChannelOperationKind::Receive
        })
    }

    fn plan_channel_operand(&mut self, receive_expression: &Expression) -> ValuePlan {
        if let Some(ChannelOperation::Receive { channel }) = channel_operation(receive_expression) {
            let plan = self.plan_value(channel, ExpressionContext::value());
            if self.facts.is_reference(&channel.get_type()) {
                return plan.map_expression(|_, value| cancel_deref_of_address(value));
            }
            return plan;
        }
        self.plan_value(receive_expression, ExpressionContext::value())
    }

    fn fresh_ok_var(&mut self) -> GoIdentifier {
        if self.scope.has_binding_for_go_name("ok") || self.shadows_declaration("ok") {
            self.fresh_local("ok")
        } else {
            GoIdentifier::local("ok".to_string(), self.scope.new_local_id())
        }
    }

    fn lower_ok_check(
        &mut self,
        ok_var: &GoIdentifier,
        ctx: &SelectReceiveContext,
    ) -> Vec<Statement> {
        let body_block = self.lower_block_to_place(ctx.body, ctx.place);
        let body_empty = body_block.renders_empty();
        let else_block = retry_closed_channel(ctx.channel);

        let ok = GoExpression::identifier(ok_var.clone());
        let plan = if body_empty {
            IfPlan::plain(
                GoExpression::unary(UnaryOp::Not, ok),
                else_block,
                ElseArm::None,
            )
        } else {
            let else_arm = ElseArm::block(else_block);
            IfPlan::plain(ok, body_block, else_arm)
        };

        vec![LoweredStatement::If(plan).into()]
    }

    fn lower_ok_guard<F>(
        &mut self,
        prepare_receiver: F,
        inner_pattern: Option<&Pattern>,
        ctx: &SelectReceiveContext,
    ) -> SelectArmKind
    where
        F: FnOnce(&mut Self) -> GoIdentifier,
    {
        let (receiver_var, ok_var, then_statements) = self.with_binding_frame(|this| {
            let receiver_var = prepare_receiver(this);
            let ok_var = this.fresh_ok_var();
            let body_statements = if let Some(pattern) = inner_pattern {
                let receiver_subject = GoExpression::identifier(receiver_var.clone());
                this.lower_select_receive_pattern_site(
                    TypedSubject {
                        var: &receiver_subject,
                        ty: &ctx.element_ty,
                    },
                    pattern,
                    ctx.body,
                    ctx.default_body,
                    ctx.place,
                )
            } else {
                this.lower_block_to_place(ctx.body, ctx.place).statements
            };

            let receiver_var = if GoUses::of(&body_statements).contains_identifier(&receiver_var) {
                receiver_var
            } else {
                GoIdentifier::blank()
            };
            (receiver_var, ok_var, body_statements)
        });

        let else_arm = ElseArm::block(retry_closed_channel(ctx.channel));
        let receive_vars = vec![receiver_var, ok_var.clone()];
        let if_plan = IfPlan::plain(
            GoExpression::identifier(ok_var),
            LoweredBlock {
                statements: then_statements,
            },
            else_arm,
        );

        SelectArmKind::Receive {
            receive_vars,
            channel: ctx.channel.clone(),
            body: LoweredBlock {
                statements: vec![LoweredStatement::If(if_plan).into()],
            },
        }
    }

    fn lower_receive_arm(
        &mut self,
        binding: &Pattern,
        ctx: &SelectReceiveContext,
    ) -> SelectArmKind {
        let effective_pattern = unwrap_some_pattern(binding);

        if is_some_pattern(binding) {
            self.lower_receive_arm_with_ok_check(effective_pattern, ctx)
        } else {
            self.lower_receive_arm_simple(effective_pattern, ctx)
        }
    }

    fn lower_receive_arm_with_ok_check(
        &mut self,
        effective_pattern: &Pattern,
        ctx: &SelectReceiveContext,
    ) -> SelectArmKind {
        if let Pattern::Identifier {
            identifier,
            binding,
            ..
        } = effective_pattern
            && let Some(go_name) = go_name_for_binding(effective_pattern)
        {
            return self.lower_ok_guard(
                |this| {
                    this.claim_block_binding(
                        identifier,
                        binding.as_slice(),
                        &go_name,
                        OuterBindings::MayShadow,
                    )
                },
                None,
                ctx,
            );
        }
        if matches!(
            effective_pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            let (ok_var, body) = self.with_binding_frame(|this| {
                let ok_var = this.fresh_ok_var();
                let body = this.lower_ok_check(&ok_var, ctx);
                (ok_var, body)
            });
            return SelectArmKind::Receive {
                receive_vars: vec![GoIdentifier::blank(), ok_var],
                channel: ctx.channel.clone(),
                body: LoweredBlock { statements: body },
            };
        }
        let receiver_var = self.fresh_local("recv");
        self.lower_ok_guard(|_| receiver_var, Some(effective_pattern), ctx)
    }

    fn lower_receive_arm_simple(
        &mut self,
        effective_pattern: &Pattern,
        ctx: &SelectReceiveContext,
    ) -> SelectArmKind {
        debug_assert!(
            matches!(effective_pattern, Pattern::WildCard { .. }),
            "internal error: the checker allows only `_` or `Some(..)` in a select receive"
        );
        let body = self.with_binding_frame(|this| this.lower_block_to_place(ctx.body, ctx.place));
        SelectArmKind::Receive {
            receive_vars: Vec::new(),
            channel: ctx.channel.clone(),
            body,
        }
    }

    fn lower_send_arm(
        &mut self,
        operation: &PreparedChannelOperation,
        body: &Expression,
        place: &PlacePlan,
    ) -> SelectArmKind {
        let block = self.lower_block_to_place(body, place);
        match operation {
            PreparedChannelOperation::Send(ch, val) => SelectArmKind::Send {
                channel: ch.clone(),
                value: val.clone(),
                body: block,
            },
            PreparedChannelOperation::Receive(ch) => SelectArmKind::Receive {
                receive_vars: Vec::new(),
                channel: ch.clone(),
                body: block,
            },
        }
    }

    fn lower_match_receive_arm(
        &mut self,
        match_arms: &[MatchArm],
        channel: &GoExpression,
        element_ty: &Type,
        place: &PlacePlan,
    ) -> SelectArmKind {
        self.with_binding_frame(|this| {
            let (receiver_var_pattern, some_arm) = match_arms
                .iter()
                .find_map(|arm| Some((some_payload_pattern(&arm.pattern)?, arm)))
                .expect("MatchReceive must have Some arm");

            let (case_var, needs_receiver_destructure) =
                this.classify_receive_var_pattern(receiver_var_pattern);
            let ok_var = this.fresh_ok_var();

            let case_subject = GoExpression::identifier(case_var.clone());
            let some_block = this.try_lower_receive_some_arm(
                some_arm,
                match_arms,
                TypedSubject {
                    var: &case_subject,
                    ty: element_ty,
                },
                needs_receiver_destructure,
                place,
            );

            let none_block =
                this.capture_scoped_block(|this| this.lower_none_arm_body(match_arms, place));
            let Some(arms_plan) = build_receive_arms_plan(&ok_var, some_block, none_block) else {
                return SelectArmKind::Receive {
                    receive_vars: Vec::new(),
                    channel: channel.clone(),
                    body: LoweredBlock {
                        statements: Vec::new(),
                    },
                };
            };

            let mut used = GoUses::default();
            used.extend_if(&arms_plan);

            let mut body_statements: Vec<Statement> = Vec::new();
            if case_var.spelling() != "_" && !used.contains_identifier(&case_var) {
                body_statements.push(discard(GoExpression::identifier(case_var.clone())));
            }
            body_statements.push(LoweredStatement::If(arms_plan).into());
            SelectArmKind::Receive {
                receive_vars: vec![case_var, ok_var],
                channel: channel.clone(),
                body: LoweredBlock {
                    statements: body_statements,
                },
            }
        })
    }

    fn try_lower_receive_some_arm(
        &mut self,
        some_arm: &MatchArm,
        match_arms: &[MatchArm],
        subject: TypedSubject<'_>,
        needs_receiver_destructure: bool,
        place: &PlacePlan,
    ) -> Option<LoweredBlock> {
        self.capture_scoped_block(|this| {
            if !needs_receiver_destructure {
                return this.lower_block_to_place(&some_arm.expression, place);
            }
            let Pattern::EnumVariant { fields, .. } = &some_arm.pattern else {
                unreachable!("Some arm must carry an EnumVariant pattern");
            };
            LoweredBlock {
                statements: this.lower_select_match_receive_some_site(
                    subject,
                    &fields[0],
                    &some_arm.expression,
                    match_arms,
                    place,
                ),
            }
        })
    }

    fn lower_select_receive_pattern_site(
        &mut self,
        subject: TypedSubject,
        pattern: &Pattern,
        body: &Expression,
        default_body: Option<&Expression>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        self.lower_refutable_arm(subject, pattern, body, place, |this| {
            default_body.map(|default| this.lower_block_to_place(default, place))
        })
    }

    fn lower_select_match_receive_some_site(
        &mut self,
        subject: TypedSubject,
        pattern: &Pattern,
        some_body: &Expression,
        match_arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Vec<Statement> {
        self.lower_refutable_arm(subject, pattern, some_body, place, |this| {
            Some(this.lower_none_arm_body(match_arms, place))
        })
    }

    fn lower_refutable_arm(
        &mut self,
        subject: TypedSubject,
        pattern: &Pattern,
        body: &Expression,
        place: &PlacePlan,
        failure: impl FnOnce(&mut Planner) -> Option<LoweredBlock>,
    ) -> Vec<Statement> {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let info = self.collect_pattern_info(pattern, subject_ty);
        let mut statements = Vec::new();
        let asserted = self.apply_refutable_root_assertion(&mut statements, info, subject_var);

        if asserted.is_irrefutable() {
            self.with_tree_bindings(
                &mut statements,
                &asserted.bindings,
                &asserted.subject,
                body,
                |this, statements| {
                    let block = this.lower_block_to_place(body, place);
                    statements.extend(block.statements);
                },
            );
            return statements;
        }
        let condition = asserted.condition(&mut self.scope);
        let mut then_body = Vec::new();
        self.with_tree_bindings(
            &mut then_body,
            &asserted.bindings,
            &asserted.subject,
            body,
            |this, then_body| {
                let block = this.lower_block_to_place(body, place);
                then_body.extend(block.statements);
            },
        );
        let else_arm = match failure(self) {
            Some(body) => ElseArm::block(body),
            None => ElseArm::None,
        };
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                condition,
                LoweredBlock {
                    statements: then_body,
                },
                else_arm,
            ))
            .into(),
        );
        statements
    }

    fn classify_receive_var_pattern(&mut self, pattern: &Pattern) -> (GoIdentifier, bool) {
        match pattern {
            Pattern::WildCard { .. } => (GoIdentifier::blank(), false),
            Pattern::Identifier {
                identifier,
                binding,
                ..
            } => {
                let Some(go_name) = go_name_for_binding(pattern) else {
                    return (GoIdentifier::blank(), false);
                };
                let preferred = go_name.preferred();
                if self.scope.resolve_identifier_binding(identifier).is_some()
                    || self
                        .scope
                        .go_name_bound_by_other_lisette_name(&preferred, identifier)
                    || self.names.is_package_block_name(&preferred)
                {
                    let name = self.fresh_local("recv");
                    return (name, true);
                }
                (
                    self.scope
                        .bind_new_local(identifier, binding.as_slice(), go_name),
                    false,
                )
            }
            _ => {
                let name = self.fresh_local("recv");
                (name, true)
            }
        }
    }

    fn lower_none_arm_body(&mut self, match_arms: &[MatchArm], place: &PlacePlan) -> LoweredBlock {
        for match_arm in match_arms {
            if matches!(
                prelude_variant(&match_arm.pattern),
                Some((PreludeVariant::None, _))
            ) {
                return self.lower_block_to_place(&match_arm.expression, place);
            }
        }
        LoweredBlock {
            statements: Vec::new(),
        }
    }
}

fn retry_closed_channel(channel: &GoExpression) -> LoweredBlock {
    LoweredBlock {
        statements: vec![
            assign(channel.clone(), GoExpression::nil()),
            LoweredStatement::Continue(LoopTransfer::Unlabeled).into(),
        ],
    }
}

fn cancel_deref_of_address(channel: GoExpression) -> GoExpression {
    match channel.node() {
        GoExpressionNode::AddressOf(inner) => channel.peeled(inner),
        _ => GoExpression::dereference(channel),
    }
}

fn build_receive_arms_plan(
    ok_var: &GoIdentifier,
    some: Option<LoweredBlock>,
    none: Option<LoweredBlock>,
) -> Option<IfPlan> {
    let ok = || GoExpression::identifier(ok_var.clone());
    match (some, none) {
        (Some(some), Some(none)) => Some(IfPlan::plain(ok(), some, ElseArm::block(none))),
        (Some(some), None) => Some(IfPlan::plain(ok(), some, ElseArm::None)),
        (None, Some(none)) => Some(IfPlan::plain(
            GoExpression::unary(UnaryOp::Not, ok()),
            none,
            ElseArm::None,
        )),
        (None, None) => None,
    }
}

fn unwrap_some_pattern(pattern: &Pattern) -> &Pattern {
    let pattern = peel_as_binding(pattern);
    some_payload_pattern(pattern).unwrap_or(pattern)
}

fn some_payload_pattern(pattern: &Pattern) -> Option<&Pattern> {
    match prelude_variant(pattern)? {
        (PreludeVariant::Some, [payload]) => Some(payload),
        _ => None,
    }
}

fn is_some_pattern(pattern: &Pattern) -> bool {
    some_payload_pattern(peel_as_binding(pattern)).is_some()
}

fn peel_as_binding(pattern: &Pattern) -> &Pattern {
    match pattern {
        Pattern::AsBinding { pattern, .. } => pattern.as_ref(),
        p => p,
    }
}
