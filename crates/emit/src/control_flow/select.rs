use crate::OuterBindings;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::patterns::sites::{
    self, AnnotatedPattern, PatternSubject, TypedSubject, is_some_pattern, unwrap_some_pattern,
};
use crate::plan::bodies::GoUses;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    PlacePlan, SelectArmPlan, SelectStatementPlan, Statement, assign, discard,
};
use crate::plan::go_expression::{GoExpressionNode, UnaryOp};
use crate::plan::local::GoIdentifier;
use crate::plan::placement::unreachable_panic_if_needed;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm};
use syntax::program::{ChannelOperation, channel_operation};
use syntax::types::Type;

enum PreparedChannelOperation {
    Send(GoExpression, GoExpression),
    Receive(GoExpression),
}

struct SelectReceiveContext<'a> {
    channel: &'a GoExpression,
    body: &'a Expression,
    default_body: Option<&'a Expression>,
    retry_var: Option<&'a GoExpression>,
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
    },
    MatchReceive {
        arms: &'a [MatchArm],
        channel: GoExpression,
        element_ty: Type,
    },
    Default {
        body: &'a Expression,
    },
}

impl Planner<'_> {
    pub(crate) fn lower_select(
        &mut self,
        arms: &[SelectArm],
        place: &PlacePlan,
    ) -> LoweredStatement {
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
            statements.push(LoweredStatement::Loop(loop_plan).into());
            statements.extend(postlude);
            LoweredStatement::Body(LoweredBlock { statements })
        } else {
            let mut statements = setup;
            statements.push(select.into());
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
            PreparedSelectArm::Default { body } => Some(*body),
            _ => None,
        });

        let mut arm_plans = Vec::with_capacity(arms.len());
        for arm in arms {
            let plan = match arm {
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
                        retry_var: is_some_pattern(binding).then_some(&channel),
                        element_ty,
                        place,
                    };
                    self.lower_receive_arm(binding, &receiver_ctx)
                }
                PreparedSelectArm::Send { body, operation } => {
                    self.lower_send_arm(&operation, body, place)
                }
                PreparedSelectArm::MatchReceive {
                    arms,
                    channel,
                    element_ty,
                } => self.lower_match_receive_arm(arms, &channel, &element_ty, place),
                PreparedSelectArm::Default { body } => SelectArmPlan::Default {
                    body: self.lower_block_to_place(body, place),
                },
            };
            arm_plans.push(plan);
        }
        arm_plans
    }

    /// Hoist all side-effectful arm expressions into temps so they evaluate
    /// in source order, not on each retry.
    fn preprocess_select_arms<'a>(
        &mut self,
        setup: &mut Vec<Statement>,
        arms: &'a [SelectArm],
        needs_retry_loop: bool,
    ) -> Vec<PreparedSelectArm<'a>> {
        let mut prepared = Vec::with_capacity(arms.len());

        for arm in arms {
            let prepared_arm = match arm {
                SelectArm::Send {
                    send_expression,
                    body,
                } => PreparedSelectArm::Send {
                    body,
                    operation: self.prepare_send_arm(setup, send_expression, needs_retry_loop),
                },
                SelectArm::Receive {
                    receive_expression,
                    binding,
                    body,
                    ..
                } => {
                    let channel = self.lower_channel_operand(receive_expression);
                    let repeats = channel.effects().can_duplicate();
                    let (channel_setup, channel_value) = channel.into_parts();
                    setup.extend(channel_setup);
                    let channel = if is_some_pattern(binding) || (needs_retry_loop && !repeats) {
                        GoExpression::name(self.hoist_tmp_value_statement(
                            setup,
                            "ch",
                            channel_value,
                        ))
                    } else {
                        channel_value
                    };
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
                } => {
                    let channel = self.lower_channel_operand(receive_expression);
                    let repeats = channel.effects().can_duplicate();
                    let (channel_setup, channel_value) = channel.into_parts();
                    setup.extend(channel_setup);
                    let channel = if needs_retry_loop && !repeats {
                        GoExpression::name(self.hoist_tmp_value_statement(
                            setup,
                            "ch",
                            channel_value,
                        ))
                    } else {
                        channel_value
                    };
                    PreparedSelectArm::MatchReceive {
                        arms,
                        channel,
                        element_ty: receive_expression.get_type().ok_type(),
                    }
                }
                SelectArm::WildCard { body } => PreparedSelectArm::Default { body },
            };
            prepared.push(prepared_arm);
        }

        prepared
    }

    fn lower_channel_operand(&mut self, receive_expression: &Expression) -> ValuePlan {
        let unwrapped = receive_expression.unwrap_parens();
        if let Some(ChannelOperation::Receive { channel }) = channel_operation(unwrapped) {
            let plan = self.lower_value(channel, ExpressionContext::value());
            if channel.get_type().is_ref() {
                return plan.map_expression(|_, value| cancel_deref_of_address(value));
            }
            return plan;
        }
        self.lower_value(receive_expression, ExpressionContext::value())
    }

    fn fresh_ok_var(&mut self) -> GoIdentifier {
        if self.scope.has_binding_for_go_name("ok") || self.shadows_declaration("ok") {
            let name = self.fresh_var(Some("ok"));
            self.scope.generated_identifier(&name)
        } else {
            GoIdentifier::local("ok".to_string(), self.scope.new_local_id())
        }
    }

    fn lower_ok_check(
        &mut self,
        ok_var: &GoIdentifier,
        ctx: &SelectReceiveContext,
    ) -> Vec<Statement> {
        // Decide scaffolding on rendered emptiness, not `is_empty`: some lowered
        // statements (e.g. a discard `let _`) render to empty text even when the
        // IR is structurally non-empty.
        let body_block = self.lower_block_to_place(ctx.body, ctx.place);
        let body_empty = body_block.renders_empty();
        let else_block = self.build_ok_else_block(ctx);
        let has_else = else_block.is_some();

        if body_empty && !has_else {
            return Vec::new();
        }

        let ok = GoExpression::identifier(ok_var.clone());
        let plan = if body_empty {
            IfPlan::plain(
                GoExpression::unary(UnaryOp::Not, ok),
                else_block.expect("body_empty && has_else"),
                ElseArm::None,
            )
        } else {
            let else_arm = match else_block {
                Some(body) => ElseArm::from_body(body, false),
                None => ElseArm::None,
            };
            IfPlan::plain(ok, body_block, else_arm)
        };
        vec![LoweredStatement::If(plan).into()]
    }

    /// Else branch for an ok-check: retry (`v = nil; continue`) or default
    /// body. `None` when neither applies or the default lowers empty.
    fn build_ok_else_block(&mut self, ctx: &SelectReceiveContext) -> Option<LoweredBlock> {
        if let Some(retry_var) = ctx.retry_var {
            return Some(LoweredBlock {
                statements: vec![
                    assign(retry_var.clone(), GoExpression::nil()),
                    LoweredStatement::Continue(LoopTransfer::Unlabeled).into(),
                ],
            });
        }
        let default_body = ctx.default_body?;
        let block = self.lower_block_to_place(default_body, ctx.place);
        (!block.is_empty()).then_some(block)
    }

    /// `case v, ok := <-ch:` plus an `if ok { ... } else { ... }` body.
    fn lower_ok_guard<F>(
        &mut self,
        prepare_receiver: F,
        inner_pattern: Option<&Pattern>,
        ctx: &SelectReceiveContext,
    ) -> SelectArmPlan
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
                    AnnotatedPattern { pattern },
                    ctx.body,
                    ctx.default_body,
                    ctx.place,
                )
            } else {
                this.lower_block_to_place(ctx.body, ctx.place).statements
            };
            let mut then_statements: Vec<Statement> = Vec::new();
            if !GoUses::of(&body_statements).contains_identifier(&receiver_var) {
                then_statements.push(discard(GoExpression::identifier(receiver_var.clone())));
            }
            then_statements.extend(body_statements);
            (receiver_var, ok_var, then_statements)
        });

        let else_arm = match self.build_ok_else_block(ctx) {
            Some(body) => ElseArm::from_body(body, false),
            None => ElseArm::None,
        };
        let receive_vars = vec![receiver_var, ok_var.clone()];
        let if_plan = IfPlan::plain(
            GoExpression::identifier(ok_var),
            LoweredBlock {
                statements: then_statements,
            },
            else_arm,
        );
        SelectArmPlan::Receive {
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
    ) -> SelectArmPlan {
        let effective_pattern = unwrap_some_pattern(binding);

        if is_some_pattern(binding) {
            self.lower_receive_arm_with_ok_check(effective_pattern, ctx)
        } else {
            self.lower_receive_arm_simple(effective_pattern, ctx)
        }
    }

    /// `case x, ok := <-ch:` with an `if ok` guard or `if !ok { break }`.
    fn lower_receive_arm_with_ok_check(
        &mut self,
        effective_pattern: &Pattern,
        ctx: &SelectReceiveContext,
    ) -> SelectArmPlan {
        if let Pattern::Identifier {
            identifier,
            binding,
            ..
        } = effective_pattern
            && let Some(go_name) = self.go_name_for_binding(effective_pattern)
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
            return SelectArmPlan::Receive {
                receive_vars: vec![GoIdentifier::name("_".to_string()), ok_var],
                channel: ctx.channel.clone(),
                body: LoweredBlock { statements: body },
            };
        }
        let receiver_var = self.fresh_var(Some("recv"));
        let receiver_var = self.scope.generated_identifier(&receiver_var);
        self.lower_ok_guard(|_| receiver_var, Some(effective_pattern), ctx)
    }

    /// Plain receive: `case v := <-ch:` then the arm body.
    fn lower_receive_arm_simple(
        &mut self,
        effective_pattern: &Pattern,
        ctx: &SelectReceiveContext,
    ) -> SelectArmPlan {
        self.with_binding_frame(|this| {
            let mut body_statements: Vec<Statement> = Vec::new();
            let receive_vars = if let Pattern::Identifier {
                identifier,
                binding,
                ..
            } = effective_pattern
                && let Some(go_name) = this.go_name_for_binding(effective_pattern)
            {
                vec![this.claim_block_binding(
                    identifier,
                    binding.as_slice(),
                    &go_name,
                    OuterBindings::MayShadow,
                )]
            } else if matches!(
                effective_pattern,
                Pattern::Identifier { .. } | Pattern::WildCard { .. }
            ) {
                Vec::new()
            } else {
                let receiver_var = this.fresh_var(Some("recv"));
                body_statements.extend(this.lower_irrefutable_pattern_site(
                    PatternSubject::for_value(receiver_var.clone()),
                    effective_pattern,
                    &ctx.element_ty,
                ));
                vec![this.scope.generated_identifier(&receiver_var)]
            };
            let block = this.lower_block_to_place(ctx.body, ctx.place);
            body_statements.extend(block.statements);
            SelectArmPlan::Receive {
                receive_vars,
                channel: ctx.channel.clone(),
                body: LoweredBlock {
                    statements: body_statements,
                },
            }
        })
    }

    fn prepare_send_arm(
        &mut self,
        setup: &mut Vec<Statement>,
        send_expression: &Expression,
        needs_hoist: bool,
    ) -> PreparedChannelOperation {
        let unwrapped = send_expression.unwrap_parens();
        if let Some(operation) = channel_operation(unwrapped) {
            let channel = operation.channel();
            let channel_plan = self.lower_value(channel, ExpressionContext::value());
            let pin_channel = needs_hoist && !channel_plan.effects().can_duplicate();
            let (channel_setup, mut channel_value) = channel_plan.into_parts();
            setup.extend(channel_setup);
            if channel.get_type().is_ref() {
                channel_value = cancel_deref_of_address(channel_value);
            }
            if pin_channel {
                channel_value =
                    GoExpression::name(self.hoist_tmp_value_statement(setup, "ch", channel_value));
            }
            match operation {
                ChannelOperation::Send { value, .. } => {
                    let value_plan = self.lower_composite_value(value, ExpressionContext::value());
                    let pin_value = needs_hoist && !value_plan.effects().can_duplicate();
                    let (value_setup, mut sent) = value_plan.into_parts();
                    setup.extend(value_setup);
                    if pin_value {
                        sent = GoExpression::name(
                            self.hoist_tmp_value_statement(setup, "send_val", sent),
                        );
                    }
                    PreparedChannelOperation::Send(channel_value, sent)
                }
                ChannelOperation::Receive { .. } => {
                    PreparedChannelOperation::Receive(channel_value)
                }
            }
        } else {
            let expression_plan = self.lower_value(send_expression, ExpressionContext::value());
            let pin_channel = needs_hoist && !expression_plan.effects().can_duplicate();
            let (expression_setup, mut channel_value) = expression_plan.into_parts();
            setup.extend(expression_setup);
            if send_expression.get_type().is_ref() {
                channel_value = cancel_deref_of_address(channel_value);
            }
            if pin_channel {
                channel_value =
                    GoExpression::name(self.hoist_tmp_value_statement(setup, "ch", channel_value));
            }
            PreparedChannelOperation::Receive(channel_value)
        }
    }

    /// `case ch <- v:` or a bare `case <-ch:` plus the arm body.
    fn lower_send_arm(
        &mut self,
        operation: &PreparedChannelOperation,
        body: &Expression,
        place: &PlacePlan,
    ) -> SelectArmPlan {
        let block = self.lower_block_to_place(body, place);
        match operation {
            PreparedChannelOperation::Send(ch, val) => SelectArmPlan::Send {
                channel: ch.clone(),
                value: val.clone(),
                body: block,
            },
            PreparedChannelOperation::Receive(ch) => SelectArmPlan::Receive {
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
    ) -> SelectArmPlan {
        self.with_binding_frame(|this| {
            let (receiver_var_pattern, some_arm) = match_arms
                .iter()
                .find_map(|arm| Some((sites::some_payload_pattern(&arm.pattern)?, arm)))
                .expect("MatchReceive must have Some arm");

            let (case_var, needs_receiver_destructure) =
                this.classify_receive_var_pattern(receiver_var_pattern);
            let ok_var = this.fresh_ok_var();

            let case_subject = GoExpression::identifier(case_var.clone());
            let some_block = this.lower_receive_some_arm(
                some_arm,
                match_arms,
                TypedSubject {
                    var: &case_subject,
                    ty: element_ty,
                },
                needs_receiver_destructure,
                place,
            );
            let none_block = this
                .capture_scoped_block(|this| sites::lower_none_arm_body(this, match_arms, place));
            let arms_plan = build_receive_arms_plan(&ok_var, some_block, none_block);
            let mut used = GoUses::default();
            if let Some(plan) = &arms_plan {
                used.extend_if(plan);
            }

            // Per-var discards (emitted when the body does not reference the var)
            // precede the structured body inside the `case x, ok := <-ch:` arm.
            let mut body_statements: Vec<Statement> = Vec::new();
            if !used.contains_identifier(&ok_var) {
                body_statements.push(discard(GoExpression::identifier(ok_var.clone())));
            }
            if case_var != "_" && !used.contains_identifier(&case_var) {
                body_statements.push(discard(GoExpression::identifier(case_var.clone())));
            }
            if let Some(plan) = arms_plan {
                body_statements.push(LoweredStatement::If(plan).into());
            }
            SelectArmPlan::Receive {
                receive_vars: vec![case_var, ok_var],
                channel: channel.clone(),
                body: LoweredBlock {
                    statements: body_statements,
                },
            }
        })
    }

    /// Lower the Some arm body (with payload destructure) so the caller can
    /// wrap it in `if ok` alongside the None arm. `None` if it renders empty.
    fn lower_receive_some_arm(
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
                    AnnotatedPattern {
                        pattern: &fields[0],
                    },
                    &some_arm.expression,
                    match_arms,
                    place,
                ),
            }
        })
    }
}

/// `*&x` is `x`. Any other pointer gets dereferenced.
fn cancel_deref_of_address(channel: GoExpression) -> GoExpression {
    let addressed = match channel.node() {
        GoExpressionNode::AddressOf(inner) => Some(inner.as_ref()),
        _ => None,
    };
    match addressed {
        Some(inner) => GoExpression::from_node(inner.clone()),
        None => GoExpression::dereference(channel),
    }
}

/// `if ok { Some } else { None }`, collapsing to `if ok`/`if !ok`/`None`
/// when one or both arms are empty.
fn build_receive_arms_plan(
    ok_var: &GoIdentifier,
    some: Option<LoweredBlock>,
    none: Option<LoweredBlock>,
) -> Option<IfPlan> {
    let ok = || GoExpression::identifier(ok_var.clone());
    match (some, none) {
        (Some(some), Some(none)) => {
            Some(IfPlan::plain(ok(), some, ElseArm::from_body(none, false)))
        }
        (Some(some), None) => Some(IfPlan::plain(ok(), some, ElseArm::None)),
        (None, Some(none)) => Some(IfPlan::plain(
            GoExpression::unary(UnaryOp::Not, ok()),
            none,
            ElseArm::None,
        )),
        (None, None) => None,
    }
}
