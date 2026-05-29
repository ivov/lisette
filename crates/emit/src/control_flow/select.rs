use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::ReturnContext;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::patterns::sites::{
    self, AnnotatedPattern, PatternSubject, TypedSubject, is_some_pattern, unwrap_some_pattern,
    unwrap_some_typed_pattern,
};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, SelectArmPlan, SelectStatementPlan,
};
use crate::plan::placement::emit_unreachable_panic_if_needed;
use crate::utils::contains_call;
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm, SelectArmPattern, TypedPattern};
use syntax::types::unqualified_name;

enum SendArmParts {
    Send(String, String),
    Receive(String),
    Default,
}

struct SelectReceiveContext<'a> {
    channel: &'a str,
    body: &'a Expression,
    default_body: Option<&'a Expression>,
    retry_var: Option<&'a str>,
    element_ty: syntax::types::Type,
    place: &'a PlacePlan<'a>,
}

struct SelectPrep {
    send_parts: Vec<Option<SendArmParts>>,
    channel_operands: Vec<Option<String>>,
    channel_shadows: Vec<Option<String>>,
}

impl Planner<'_> {
    /// Lower a `select` expression to a structured `SelectStatementPlan`.
    pub(crate) fn lower_select(
        &mut self,
        arms: &[SelectArm],
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> SelectStatementPlan {
        let needs_retry_loop = arms.iter().any(|arm| {
            matches!(&arm.pattern, SelectArmPattern::Receive { binding, .. } if is_some_pattern(binding))
        });

        let mut setup: Vec<LoweredStatement> = Vec::new();
        let mut setup_buffer = String::new();
        let prep = self.preprocess_select_arms(
            &mut setup_buffer,
            arms,
            needs_retry_loop,
            Some(place.return_ctx()),
            fx,
        );
        if !setup_buffer.is_empty() {
            setup.push(LoweredStatement::RawGo(setup_buffer));
        }

        self.enter_scope();
        let arm_plans = self.lower_select_arms(arms, &prep, place, fx);
        self.exit_scope();

        let has_default = arms
            .iter()
            .any(|arm| matches!(arm.pattern, SelectArmPattern::WildCard { .. }));
        let all_arms_diverge =
            !arm_plans.is_empty() && arm_plans.iter().all(|arm| arm.body().ends_with_diverge());
        let exhaustive = all_arms_diverge || if needs_retry_loop { false } else { has_default };
        let mut postlude: Vec<LoweredStatement> = Vec::new();
        let mut panic_buffer = String::new();
        emit_unreachable_panic_if_needed(&mut panic_buffer, place, exhaustive);
        if !panic_buffer.is_empty() {
            postlude.push(LoweredStatement::RawGo(panic_buffer));
        }

        SelectStatementPlan {
            directive: String::new(),
            setup,
            retry_loop: needs_retry_loop,
            arms: arm_plans,
            postlude,
        }
    }

    fn lower_select_arms(
        &mut self,
        arms: &[SelectArm],
        prep: &SelectPrep,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> Vec<SelectArmPlan> {
        let default_body = arms.iter().find_map(|arm| {
            if let SelectArmPattern::WildCard { body } = &arm.pattern {
                Some(body.as_ref())
            } else {
                None
            }
        });

        let mut arm_plans = Vec::with_capacity(arms.len());
        for (i, arm) in arms.iter().enumerate() {
            let plan = match &arm.pattern {
                SelectArmPattern::Receive {
                    binding,
                    typed_pattern,
                    receive_expression,
                    body,
                } => {
                    let (channel, retry_var) = if let Some(shadow) =
                        prep.channel_shadows.get(i).and_then(|s| s.as_ref())
                    {
                        (shadow.as_str(), Some(shadow.as_str()))
                    } else {
                        (prep.channel_operands[i].as_ref().unwrap().as_str(), None)
                    };
                    let receiver_ctx = SelectReceiveContext {
                        channel,
                        body,
                        default_body,
                        retry_var,
                        element_ty: receive_expression.get_type().ok_type(),
                        place,
                    };
                    self.lower_receive_arm(binding, typed_pattern.as_ref(), &receiver_ctx, fx)
                }
                SelectArmPattern::Send { body, .. } => {
                    let parts = prep.send_parts[i].as_ref().unwrap();
                    self.lower_send_arm(parts, body, place, fx)
                }
                SelectArmPattern::MatchReceive {
                    arms: match_arms,
                    receive_expression,
                } => {
                    let channel = prep.channel_operands[i].as_ref().unwrap();
                    let element_ty = receive_expression.get_type().ok_type();
                    self.lower_match_receive_arm(match_arms, channel, &element_ty, place, fx)
                }
                SelectArmPattern::WildCard { body } => SelectArmPlan::Default {
                    body: self.lower_block_to_place(body, place, fx),
                },
            };
            arm_plans.push(plan);
        }
        arm_plans
    }

    /// Hoist all side-effectful arm expressions into temps so they evaluate
    /// in source order, not on each retry.
    fn preprocess_select_arms(
        &mut self,
        output: &mut String,
        arms: &[SelectArm],
        needs_retry_loop: bool,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> SelectPrep {
        let mut send_parts: Vec<Option<SendArmParts>> = Vec::with_capacity(arms.len());
        let mut channel_operands: Vec<Option<String>> = Vec::with_capacity(arms.len());
        let mut channel_shadows: Vec<Option<String>> = Vec::with_capacity(arms.len());

        for arm in arms.iter() {
            match &arm.pattern {
                SelectArmPattern::Send {
                    send_expression, ..
                } => {
                    let parts = self.prepare_send_arm(
                        output,
                        send_expression,
                        needs_retry_loop,
                        ambient,
                        fx,
                    );
                    send_parts.push(Some(parts));
                    channel_operands.push(None);
                    channel_shadows.push(None);
                }
                SelectArmPattern::Receive {
                    receive_expression,
                    binding,
                    ..
                } => {
                    let channel_has_call = channel_expression_has_call(receive_expression);
                    let ch = self.emit_channel_operand(output, receive_expression, ambient, fx);
                    if is_some_pattern(binding) && needs_retry_loop {
                        let shadow = self.hoist_tmp_value(output, "ch", &ch);
                        channel_operands.push(Some(ch));
                        channel_shadows.push(Some(shadow));
                    } else {
                        let ch = if needs_retry_loop && channel_has_call {
                            self.hoist_tmp_value(output, "ch", &ch)
                        } else {
                            ch
                        };
                        channel_operands.push(Some(ch));
                        channel_shadows.push(None);
                    }
                    send_parts.push(None);
                }
                SelectArmPattern::MatchReceive {
                    receive_expression, ..
                } => {
                    let channel_has_call = channel_expression_has_call(receive_expression);
                    let ch = self.emit_channel_operand(output, receive_expression, ambient, fx);
                    let ch = if needs_retry_loop && channel_has_call {
                        self.hoist_tmp_value(output, "ch", &ch)
                    } else {
                        ch
                    };
                    channel_operands.push(Some(ch));
                    send_parts.push(None);
                    channel_shadows.push(None);
                }
                SelectArmPattern::WildCard { .. } => {
                    send_parts.push(None);
                    channel_operands.push(None);
                    channel_shadows.push(None);
                }
            }
        }

        SelectPrep {
            send_parts,
            channel_operands,
            channel_shadows,
        }
    }

    fn emit_channel_operand(
        &mut self,
        output: &mut String,
        receive_expression: &Expression,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> String {
        let unwrapped = receive_expression.unwrap_parens();
        if let Some((channel, "receive", _)) = extract_channel_op(unwrapped) {
            let ch = self.emit_value(
                output,
                channel,
                ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                fx,
            );
            return if channel.get_type().is_ref() {
                cancel_deref_of_address(ch)
            } else {
                ch
            };
        }
        self.emit_value(
            output,
            receive_expression,
            ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
            fx,
        )
    }

    fn fresh_ok_var(&mut self) -> String {
        if self.scope.has_binding_for_go_name("ok") || self.is_declared("ok") {
            self.fresh_var(Some("ok"))
        } else {
            "ok".to_string()
        }
    }

    fn lower_ok_check(
        &mut self,
        ok_var: &str,
        ctx: &SelectReceiveContext,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        // Decide scaffolding on rendered emptiness, not `is_empty`: some lowered
        // statements (e.g. a discard `let _`) render to empty text even when the
        // IR is structurally non-empty.
        let body_block = self.lower_block_to_place(ctx.body, ctx.place, fx);
        let body_empty = Renderer.renders_empty(&body_block);
        let else_block = self.build_ok_else_block(ctx, fx);
        let has_else = else_block.is_some();

        if body_empty && !has_else {
            return Vec::new();
        }

        let plan = if body_empty {
            IfPlan {
                directive: String::new(),
                condition_setup: String::new(),
                condition: format!("!{}", ok_var),
                then_body: else_block.expect("body_empty && has_else"),
                else_arm: ElseArm::None,
            }
        } else {
            let else_arm = match else_block {
                Some(body) => ElseArm::Else {
                    body,
                    inline: false,
                },
                None => ElseArm::None,
            };
            IfPlan {
                directive: String::new(),
                condition_setup: String::new(),
                condition: ok_var.to_string(),
                then_body: body_block,
                else_arm,
            }
        };
        vec![LoweredStatement::If(plan)]
    }

    /// Else branch for an ok-check: retry (`v = nil; continue`) or default
    /// body. `None` when neither applies or the default lowers empty.
    fn build_ok_else_block(
        &mut self,
        ctx: &SelectReceiveContext,
        fx: &mut EmitEffects,
    ) -> Option<LoweredBlock> {
        if let Some(retry_var) = ctx.retry_var {
            return Some(LoweredBlock {
                statements: vec![
                    LoweredStatement::RawGo(format!("{} = nil\n", retry_var)),
                    LoweredStatement::Continue {
                        directive: String::new(),
                        label: None,
                    },
                ],
            });
        }
        let default_body = ctx.default_body?;
        let block = self.lower_block_to_place(default_body, ctx.place, fx);
        (!block.is_empty()).then_some(block)
    }

    /// `case v, ok := <-ch:` plus an `if ok { ... } else { ... }` body.
    fn lower_ok_guard(
        &mut self,
        receiver_var: &str,
        inner_pattern: Option<(&Pattern, Option<&TypedPattern>)>,
        ctx: &SelectReceiveContext,
        fx: &mut EmitEffects,
    ) -> SelectArmPlan {
        let ok_var = self.fresh_ok_var();
        let receive_vars = format!("{}, {}", receiver_var, ok_var);
        let body_statements = if let Some((pattern, typed)) = inner_pattern {
            self.lower_select_receive_pattern_site(
                TypedSubject {
                    var: receiver_var,
                    ty: &ctx.element_ty,
                },
                AnnotatedPattern { pattern, typed },
                ctx.body,
                ctx.default_body,
                ctx.place,
                fx,
            )
        } else {
            self.lower_block_to_place(ctx.body, ctx.place, fx)
                .statements
        };
        let body_holder = LoweredBlock {
            statements: body_statements,
        };
        let mut then_statements: Vec<LoweredStatement> = Vec::new();
        if !body_holder.references_var(receiver_var) {
            then_statements.push(LoweredStatement::RawGo(format!("_ = {}\n", receiver_var)));
        }
        then_statements.extend(body_holder.statements);
        self.scope.pop_binding_frame();

        let else_arm = match self.build_ok_else_block(ctx, fx) {
            Some(body) => ElseArm::Else {
                body,
                inline: false,
            },
            None => ElseArm::None,
        };
        let if_plan = IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: ok_var,
            then_body: LoweredBlock {
                statements: then_statements,
            },
            else_arm,
        };
        SelectArmPlan::Receive {
            receive_vars: Some(receive_vars),
            channel: ctx.channel.to_string(),
            body: LoweredBlock {
                statements: vec![LoweredStatement::If(if_plan)],
            },
        }
    }

    fn lower_receive_arm(
        &mut self,
        binding: &Pattern,
        typed_pattern: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
        fx: &mut EmitEffects,
    ) -> SelectArmPlan {
        let effective_pattern = unwrap_some_pattern(binding);
        let inner_typed = unwrap_some_typed_pattern(typed_pattern);

        self.scope.push_binding_frame();
        if is_some_pattern(binding) {
            self.lower_receive_arm_with_ok_check(effective_pattern, inner_typed, ctx, fx)
        } else {
            self.lower_receive_arm_simple(effective_pattern, inner_typed, ctx, fx)
        }
    }

    /// `case x, ok := <-ch:` with an `if ok` guard or `if !ok { break }`.
    fn lower_receive_arm_with_ok_check(
        &mut self,
        effective_pattern: &Pattern,
        inner_typed: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
        fx: &mut EmitEffects,
    ) -> SelectArmPlan {
        if let Pattern::Identifier { identifier, .. } = effective_pattern
            && let Some(go_name) = self.go_name_for_binding(effective_pattern)
        {
            let var = self.scope.bind(identifier, go_name);
            return self.lower_ok_guard(&var, None, ctx, fx);
        }
        if matches!(
            effective_pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            let ok_var = self.fresh_ok_var();
            let body = self.lower_ok_check(&ok_var, ctx, fx);
            self.scope.pop_binding_frame();
            return SelectArmPlan::Receive {
                receive_vars: Some(format!("_, {}", ok_var)),
                channel: ctx.channel.to_string(),
                body: LoweredBlock { statements: body },
            };
        }
        let receiver_var = self.fresh_var(Some("recv"));
        self.lower_ok_guard(
            &receiver_var,
            Some((effective_pattern, inner_typed)),
            ctx,
            fx,
        )
    }

    /// Plain receive: `case v := <-ch:` then the arm body.
    fn lower_receive_arm_simple(
        &mut self,
        effective_pattern: &Pattern,
        inner_typed: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
        fx: &mut EmitEffects,
    ) -> SelectArmPlan {
        let mut body_statements: Vec<LoweredStatement> = Vec::new();
        let receive_vars = if let Pattern::Identifier { identifier, .. } = effective_pattern
            && let Some(go_name) = self.go_name_for_binding(effective_pattern)
        {
            Some(self.scope.bind(identifier, go_name))
        } else if matches!(
            effective_pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            None
        } else {
            let receiver_var = self.fresh_var(Some("recv"));
            let mut destructure = String::new();
            self.emit_irrefutable_pattern_site(
                &mut destructure,
                PatternSubject::for_value(receiver_var.clone()),
                effective_pattern,
                inner_typed,
                &ctx.element_ty,
                fx,
            );
            if !destructure.is_empty() {
                body_statements.push(LoweredStatement::RawGo(destructure));
            }
            Some(receiver_var)
        };
        let block = self.lower_block_to_place(ctx.body, ctx.place, fx);
        self.scope.pop_binding_frame();
        body_statements.extend(block.statements);
        SelectArmPlan::Receive {
            receive_vars,
            channel: ctx.channel.to_string(),
            body: LoweredBlock {
                statements: body_statements,
            },
        }
    }

    fn prepare_send_arm(
        &mut self,
        output: &mut String,
        send_expression: &Expression,
        needs_hoist: bool,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> SendArmParts {
        let unwrapped = send_expression.unwrap_parens();
        if let Some((channel, member, args)) = extract_channel_op(unwrapped) {
            let ch_has_call = needs_hoist && contains_call(channel);
            let mut ch = self.emit_value(
                output,
                channel,
                ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                fx,
            );
            if channel.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if ch_has_call {
                ch = self.hoist_tmp_value(output, "ch", &ch);
            }
            match member {
                "send" if !args.is_empty() => {
                    let val_has_call = needs_hoist && contains_call(&args[0]);
                    let mut val = self.emit_composite_value(
                        output,
                        &args[0],
                        ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                        fx,
                    );
                    if val_has_call {
                        val = self.hoist_tmp_value(output, "send_val", &val);
                    }
                    SendArmParts::Send(ch, val)
                }
                "receive" if args.is_empty() => SendArmParts::Receive(ch),
                _ => SendArmParts::Default,
            }
        } else {
            let expression_has_call = needs_hoist && contains_call(send_expression);
            let mut ch = self.emit_value(
                output,
                send_expression,
                ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                fx,
            );
            if send_expression.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if expression_has_call {
                ch = self.hoist_tmp_value(output, "ch", &ch);
            }
            SendArmParts::Receive(ch)
        }
    }

    /// `case <send>:` (or `default:`) plus the arm body.
    fn lower_send_arm(
        &mut self,
        parts: &SendArmParts,
        body: &Expression,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> SelectArmPlan {
        let block = self.lower_block_to_place(body, place, fx);
        match parts {
            SendArmParts::Send(ch, val) => SelectArmPlan::Send {
                operation: format!("{} <- {}", ch, val),
                body: block,
            },
            SendArmParts::Receive(ch) => SelectArmPlan::Send {
                operation: format!("<-{}", ch),
                body: block,
            },
            SendArmParts::Default => SelectArmPlan::Default { body: block },
        }
    }

    fn lower_match_receive_arm(
        &mut self,
        match_arms: &[MatchArm],
        channel: &str,
        element_ty: &syntax::types::Type,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> SelectArmPlan {
        self.scope.push_binding_frame();

        let (receiver_var_pattern, some_arm) = match_arms
            .iter()
            .find_map(|arm| {
                if let Pattern::EnumVariant {
                    identifier, fields, ..
                } = &arm.pattern
                    && go_name::unqualified_name(identifier) == "Some"
                    && fields.len() == 1
                {
                    Some((&fields[0], arm))
                } else {
                    None
                }
            })
            .expect("MatchReceive must have Some arm");

        let (case_var, needs_receiver_destructure) =
            self.classify_receive_var_pattern(receiver_var_pattern);

        let ok_var = self.fresh_ok_var();

        let some_block = self.lower_receive_some_arm(
            some_arm,
            match_arms,
            receiver_var_pattern,
            &case_var,
            needs_receiver_destructure,
            element_ty,
            place,
            fx,
        );
        let none_block = self
            .capture_scoped_block(|this| sites::lower_none_arm_body(this, match_arms, place, fx));

        // Compose the receive-arms body as a structured `if ok { ... } else
        // { ... }` plan so usage of `case_var` and `ok_var` can be checked
        // structurally before deciding whether to emit per-var discards.
        let body_block = LoweredBlock {
            statements: match build_receive_arms_plan(&ok_var, some_block, none_block) {
                Some(plan) => vec![LoweredStatement::If(plan)],
                None => Vec::new(),
            },
        };

        self.scope.pop_binding_frame();

        // Per-var discards (emitted when the body does not reference the var)
        // precede the structured body inside the `case x, ok := <-ch:` arm.
        let mut body_statements: Vec<LoweredStatement> = Vec::new();
        if !body_block.references_var(&ok_var) {
            body_statements.push(LoweredStatement::RawGo(format!("_ = {}\n", ok_var)));
        }
        if case_var != "_" && !body_block.references_var(&case_var) {
            body_statements.push(LoweredStatement::RawGo(format!("_ = {}\n", case_var)));
        }
        body_statements.extend(body_block.statements);
        SelectArmPlan::Receive {
            receive_vars: Some(format!("{}, {}", case_var, ok_var)),
            channel: channel.to_string(),
            body: LoweredBlock {
                statements: body_statements,
            },
        }
    }

    /// Lower the Some arm body (with payload destructure) so the caller can
    /// wrap it in `if ok` alongside the None arm. `None` if it renders empty.
    #[allow(clippy::too_many_arguments)]
    fn lower_receive_some_arm(
        &mut self,
        some_arm: &MatchArm,
        match_arms: &[MatchArm],
        receiver_var_pattern: &Pattern,
        case_var: &str,
        needs_receiver_destructure: bool,
        element_ty: &syntax::types::Type,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> Option<LoweredBlock> {
        self.capture_scoped_block(|this| {
            if !needs_receiver_destructure {
                return this.lower_block_to_place(&some_arm.expression, place, fx);
            }
            let inner_typed = unwrap_some_typed_pattern(some_arm.typed_pattern.as_ref());
            LoweredBlock {
                statements: this.lower_select_match_receive_some_site(
                    TypedSubject {
                        var: case_var,
                        ty: element_ty,
                    },
                    AnnotatedPattern {
                        pattern: receiver_var_pattern,
                        typed: inner_typed,
                    },
                    &some_arm.expression,
                    match_arms,
                    place,
                    fx,
                ),
            }
        })
    }
}

/// `*&x` → `x` (avoids redundant deref when the emitter has already
/// produced an `&`-prefixed expression).
fn cancel_deref_of_address(ch: String) -> String {
    if let Some(inner) = ch.strip_prefix("(&").and_then(|s| s.strip_suffix(')')) {
        inner.to_string()
    } else if let Some(inner) = ch.strip_prefix('&') {
        inner.to_string()
    } else {
        format!("*{}", ch)
    }
}

fn channel_expression_has_call(receive_expression: &Expression) -> bool {
    let unwrapped = receive_expression.unwrap_parens();
    if let Some((channel, "receive", _)) = extract_channel_op(unwrapped) {
        contains_call(channel)
    } else {
        contains_call(receive_expression)
    }
}

/// `if ok { Some } else { None }`, collapsing to `if ok`/`if !ok`/`None`
/// when one or both arms are empty.
fn build_receive_arms_plan(
    ok_var: &str,
    some: Option<LoweredBlock>,
    none: Option<LoweredBlock>,
) -> Option<IfPlan> {
    match (some, none) {
        (Some(some), Some(none)) => Some(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: ok_var.to_string(),
            then_body: some,
            else_arm: ElseArm::Else {
                body: none,
                inline: false,
            },
        }),
        (Some(some), None) => Some(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: ok_var.to_string(),
            then_body: some,
            else_arm: ElseArm::None,
        }),
        (None, Some(none)) => Some(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: format!("!{}", ok_var),
            then_body: none,
            else_arm: ElseArm::None,
        }),
        (None, None) => None,
    }
}

fn extract_channel_op(expression: &Expression) -> Option<(&Expression, &str, &[Expression])> {
    let Expression::Call {
        expression, args, ..
    } = expression
    else {
        return None;
    };

    if let Expression::DotAccess {
        expression: channel,
        member,
        ..
    } = expression.as_ref()
        && (member == "send" || member == "receive")
    {
        return Some((channel, member, args));
    }

    if let Expression::Identifier { value, .. } = expression.as_ref() {
        let method = unqualified_name(value);
        if (method == "send" || method == "receive") && !args.is_empty() {
            return Some((&args[0], method, &args[1..]));
        }
    }

    None
}
