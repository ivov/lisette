use crate::Emitter;
use crate::expressions::context::ExpressionContext;
use crate::names::go_name;
use crate::patterns::sites::{
    self, PatternSubject, is_some_pattern, unwrap_some_pattern, unwrap_some_typed_pattern,
};
use crate::placement::{BodyPlace, emit_unreachable_panic_if_needed};
use crate::utils::{DiscardGuard, contains_call};
use crate::write_line;
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
    place: &'a BodyPlace<'a>,
}

struct SelectPrep {
    send_parts: Vec<Option<SendArmParts>>,
    channel_operands: Vec<Option<String>>,
    channel_shadows: Vec<Option<String>>,
}

impl Emitter<'_> {
    pub(crate) fn emit_select(
        &mut self,
        output: &mut String,
        arms: &[SelectArm],
        place: &BodyPlace,
    ) {
        let needs_retry_loop = arms.iter().any(|arm| {
            matches!(&arm.pattern, SelectArmPattern::Receive { binding, .. } if is_some_pattern(binding))
        });

        let prep = self.preprocess_select_arms(output, arms, needs_retry_loop);

        if needs_retry_loop {
            output.push_str("for {\n");
        }
        self.enter_scope();
        output.push_str("select {\n");

        self.emit_select_arms(output, arms, &prep, place);

        output.push_str("}\n");
        self.exit_scope();

        if needs_retry_loop {
            output.push_str("break\n}\n");
            // Go can't see that `break` is unreachable (all select paths either
            // return or continue), so emit panic to satisfy the compiler.
            emit_unreachable_panic_if_needed(output, place, false);
        } else {
            let has_default = arms
                .iter()
                .any(|arm| matches!(arm.pattern, SelectArmPattern::WildCard { .. }));
            emit_unreachable_panic_if_needed(output, place, has_default);
        }
    }

    fn emit_select_arms(
        &mut self,
        output: &mut String,
        arms: &[SelectArm],
        prep: &SelectPrep,
        place: &BodyPlace,
    ) {
        let default_body = arms.iter().find_map(|arm| {
            if let SelectArmPattern::WildCard { body } = &arm.pattern {
                Some(body.as_ref())
            } else {
                None
            }
        });

        for (i, arm) in arms.iter().enumerate() {
            match &arm.pattern {
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
                    self.emit_receive_arm(output, binding, typed_pattern.as_ref(), &receiver_ctx);
                }
                SelectArmPattern::Send { body, .. } => {
                    let parts = prep.send_parts[i].as_ref().unwrap();
                    self.emit_send_arm_case(output, parts, body, place);
                }
                SelectArmPattern::MatchReceive {
                    arms: match_arms,
                    receive_expression,
                } => {
                    let channel = prep.channel_operands[i].as_ref().unwrap();
                    let element_ty = receive_expression.get_type().ok_type();
                    self.emit_match_receive_arm(output, match_arms, channel, &element_ty, place);
                }
                SelectArmPattern::WildCard { body } => {
                    output.push_str("default:\n");
                    self.emit_body_to_place(output, body, place);
                }
            }
        }
    }

    /// Pre-process ALL arms in source order. Side-effectful expressions
    /// (channel operands, send values) are hoisted into temps so they
    /// evaluate here — not deferred to select entry or re-evaluated on retry.
    fn preprocess_select_arms(
        &mut self,
        output: &mut String,
        arms: &[SelectArm],
        needs_retry_loop: bool,
    ) -> SelectPrep {
        let mut send_parts: Vec<Option<SendArmParts>> = Vec::with_capacity(arms.len());
        let mut channel_operands: Vec<Option<String>> = Vec::with_capacity(arms.len());
        let mut channel_shadows: Vec<Option<String>> = Vec::with_capacity(arms.len());

        for arm in arms.iter() {
            match &arm.pattern {
                SelectArmPattern::Send {
                    send_expression, ..
                } => {
                    let parts = self.prepare_send_arm(output, send_expression, needs_retry_loop);
                    send_parts.push(Some(parts));
                    channel_operands.push(None);
                    channel_shadows.push(None);
                }
                SelectArmPattern::Receive {
                    receive_expression,
                    binding,
                    ..
                } => {
                    let channel_has_call = Self::channel_expression_has_call(receive_expression);
                    let ch = self.emit_channel_operand(output, receive_expression);
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
                    let channel_has_call = Self::channel_expression_has_call(receive_expression);
                    let ch = self.emit_channel_operand(output, receive_expression);
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

    /// Check whether the channel sub-expression of a receive expression has calls.
    fn channel_expression_has_call(receive_expression: &Expression) -> bool {
        let unwrapped = receive_expression.unwrap_parens();
        if let Some((channel, "receive", _)) = Self::extract_channel_op(unwrapped) {
            contains_call(channel)
        } else {
            contains_call(receive_expression)
        }
    }

    fn emit_channel_operand(
        &mut self,
        output: &mut String,
        receive_expression: &Expression,
    ) -> String {
        let unwrapped = receive_expression.unwrap_parens();
        if let Some((channel, "receive", _)) = Self::extract_channel_op(unwrapped) {
            let ch = self.emit_value(output, channel, ExpressionContext::value());
            return if channel.get_type().is_ref() {
                cancel_deref_of_address(ch)
            } else {
                ch
            };
        }
        self.emit_value(output, receive_expression, ExpressionContext::value())
    }

    fn fresh_ok_var(&mut self) -> String {
        if self.scope.has_binding_for_go_name("ok") || self.is_declared("ok") {
            self.fresh_var(Some("ok"))
        } else {
            "ok".to_string()
        }
    }

    fn emit_ok_check(&mut self, output: &mut String, ok_var: &str, ctx: &SelectReceiveContext) {
        let (body_content, ()) = self.capture_emission(output, |this, buf| {
            this.emit_body_to_place(buf, ctx.body, ctx.place);
        });
        let body_empty = body_content.is_empty();
        let has_else = ctx.retry_var.is_some() || ctx.default_body.is_some();

        if body_empty && has_else {
            write_line!(output, "if !{} {{", ok_var);
            self.emit_ok_else(output, ctx);
            output.push_str("}\n");
        } else if body_empty {
            // Both branches empty, omit if/else entirely
        } else {
            write_line!(output, "if {} {{", ok_var);
            output.push_str(&body_content);
            if has_else {
                output.push_str("} else {\n");
                self.emit_ok_else(output, ctx);
            }
            output.push_str("}\n");
        }
    }

    /// Emit the else-branch content for an ok-check: retry logic or default body.
    fn emit_ok_else(&mut self, output: &mut String, ctx: &SelectReceiveContext) {
        if let Some(retry_var) = ctx.retry_var {
            write_line!(output, "{} = nil", retry_var);
            output.push_str("continue\n");
        } else if let Some(default_body) = ctx.default_body {
            self.emit_body_to_place(output, default_body, ctx.place);
        }
    }

    /// Emit the ok-check guard for channel receives with Option semantics.
    /// Produces: `case {receiver_var}, {ok_var} := <-{channel}: if {ok_var} { ... } else { ... }`
    fn emit_ok_guard(
        &mut self,
        output: &mut String,
        receiver_var: &str,
        inner_pattern: Option<(&Pattern, Option<&TypedPattern>)>,
        ctx: &SelectReceiveContext,
    ) {
        let ok_var = self.fresh_ok_var();
        write_line!(
            output,
            "case {}, {} := <-{}:\nif {} {{",
            receiver_var,
            ok_var,
            ctx.channel,
            ok_var
        );
        let guard = DiscardGuard::new(output, receiver_var);
        if let Some((pattern, typed)) = inner_pattern {
            self.emit_select_receive_pattern_site(
                output,
                receiver_var,
                pattern,
                typed,
                &ctx.element_ty,
                ctx.body,
                ctx.default_body,
                ctx.place,
            );
        } else {
            self.emit_body_to_place(output, ctx.body, ctx.place);
        }
        guard.finish(output);
        self.scope.pop_binding_frame();
        let has_else = ctx.retry_var.is_some() || ctx.default_body.is_some();
        if has_else {
            output.push_str("} else {\n");
            self.emit_ok_else(output, ctx);
        }
        output.push_str("}\n");
    }

    fn emit_receive_arm(
        &mut self,
        output: &mut String,
        binding: &Pattern,
        typed_pattern: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
    ) {
        let effective_pattern = unwrap_some_pattern(binding);
        let inner_typed = unwrap_some_typed_pattern(typed_pattern);

        self.scope.push_binding_frame();
        if is_some_pattern(binding) {
            self.emit_receive_arm_with_ok_check(output, effective_pattern, inner_typed, ctx);
        } else {
            self.emit_receive_arm_simple(output, effective_pattern, inner_typed, ctx);
        }
    }

    /// Option-binding receive: emits a `case x, ok := <-ch:` header and
    /// either an `if ok { ... } else { ... }` guard around the body, or a
    /// discard `if !ok { break }`. Always pops the binding frame.
    fn emit_receive_arm_with_ok_check(
        &mut self,
        output: &mut String,
        effective_pattern: &Pattern,
        inner_typed: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
    ) {
        if let Pattern::Identifier { identifier, .. } = effective_pattern
            && let Some(go_name) = self.go_name_for_binding(effective_pattern)
        {
            let var = self.scope.bind(identifier, go_name);
            self.emit_ok_guard(output, &var, None, ctx);
            return;
        }
        if matches!(
            effective_pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            let ok_var = self.fresh_ok_var();
            write_line!(output, "case _, {} := <-{}:", ok_var, ctx.channel);
            self.emit_ok_check(output, &ok_var, ctx);
            self.scope.pop_binding_frame();
            return;
        }
        let receiver_var = self.fresh_var(Some("recv"));
        self.emit_ok_guard(
            output,
            &receiver_var,
            Some((effective_pattern, inner_typed)),
            ctx,
        );
    }

    /// Plain receive (no Option semantics): emits the `case ... := <-ch:`
    /// header, then the arm body. Always pops the binding frame.
    fn emit_receive_arm_simple(
        &mut self,
        output: &mut String,
        effective_pattern: &Pattern,
        inner_typed: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
    ) {
        if let Pattern::Identifier { identifier, .. } = effective_pattern
            && let Some(go_name) = self.go_name_for_binding(effective_pattern)
        {
            let var = self.scope.bind(identifier, go_name);
            write_line!(output, "case {} := <-{}:", var, ctx.channel);
        } else if matches!(
            effective_pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            write_line!(output, "case <-{}:", ctx.channel);
        } else {
            let receiver_var = self.fresh_var(Some("recv"));
            write_line!(output, "case {} := <-{}:", receiver_var, ctx.channel);
            self.emit_irrefutable_pattern_site(
                output,
                PatternSubject::for_value(receiver_var.clone()),
                effective_pattern,
                inner_typed,
                &ctx.element_ty,
            );
        }
        self.emit_body_to_place(output, ctx.body, ctx.place);
        self.scope.pop_binding_frame();
    }

    fn prepare_send_arm(
        &mut self,
        output: &mut String,
        send_expression: &Expression,
        needs_hoist: bool,
    ) -> SendArmParts {
        let unwrapped = send_expression.unwrap_parens();
        if let Some((channel, member, args)) = Self::extract_channel_op(unwrapped) {
            let ch_has_call = needs_hoist && contains_call(channel);
            let mut ch = self.emit_value(output, channel, ExpressionContext::value());
            if channel.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if ch_has_call {
                ch = self.hoist_tmp_value(output, "ch", &ch);
            }
            match member {
                "send" if !args.is_empty() => {
                    let val_has_call = needs_hoist && contains_call(&args[0]);
                    let mut val =
                        self.emit_composite_value(output, &args[0], ExpressionContext::value());
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
            let mut ch = self.emit_value(output, send_expression, ExpressionContext::value());
            if send_expression.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if expression_has_call {
                ch = self.hoist_tmp_value(output, "ch", &ch);
            }
            SendArmParts::Receive(ch)
        }
    }

    /// Emit the `case` line and body for a pre-processed send arm.
    fn emit_send_arm_case(
        &mut self,
        output: &mut String,
        parts: &SendArmParts,
        body: &Expression,
        place: &BodyPlace,
    ) {
        match parts {
            SendArmParts::Send(ch, val) => {
                write_line!(output, "case {} <- {}:", ch, val);
            }
            SendArmParts::Receive(ch) => {
                write_line!(output, "case <-{}:", ch);
            }
            SendArmParts::Default => {
                output.push_str("default:\n");
            }
        }
        self.emit_body_to_place(output, body, place);
    }

    fn emit_match_receive_arm(
        &mut self,
        output: &mut String,
        match_arms: &[MatchArm],
        channel: &str,
        element_ty: &syntax::types::Type,
        place: &BodyPlace,
    ) {
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
        write_line!(output, "case {}, {} := <-{}:", case_var, ok_var, channel);
        let recv_guard = (case_var != "_").then(|| DiscardGuard::new(output, &case_var));
        let ok_guard = DiscardGuard::new(output, &ok_var);

        let some_content = self.render_receive_some_arm(
            output,
            some_arm,
            match_arms,
            receiver_var_pattern,
            &case_var,
            needs_receiver_destructure,
            element_ty,
            place,
        );
        let none_content = self.capture_scoped(output, |this, output| {
            sites::emit_none_arm_body(this, output, match_arms, place);
        });

        self.write_receive_arms(
            output,
            &ok_var,
            some_content.as_deref(),
            none_content.as_deref(),
        );

        if let Some(guard) = recv_guard {
            guard.finish(output);
        }
        ok_guard.finish(output);

        self.scope.pop_binding_frame();
    }

    /// Render the Some arm's body (including payload destructure when
    /// needed), returning the captured content so the caller can wrap it in
    /// an `if ok` guard alongside the None arm.
    #[allow(clippy::too_many_arguments)]
    fn render_receive_some_arm(
        &mut self,
        output: &mut String,
        some_arm: &MatchArm,
        match_arms: &[MatchArm],
        receiver_var_pattern: &Pattern,
        case_var: &str,
        needs_receiver_destructure: bool,
        element_ty: &syntax::types::Type,
        place: &BodyPlace,
    ) -> Option<String> {
        self.capture_scoped(output, |this, output| {
            if !needs_receiver_destructure {
                this.emit_body_to_place(output, &some_arm.expression, place);
                return;
            }
            let inner_typed = unwrap_some_typed_pattern(some_arm.typed_pattern.as_ref());
            this.emit_select_match_receive_some_site(
                output,
                case_var,
                receiver_var_pattern,
                inner_typed,
                element_ty,
                &some_arm.expression,
                match_arms,
                place,
            );
        })
    }

    /// Combine the rendered Some/None arm contents into `if ok { ... } else { ... }`
    /// scaffolding, collapsing to `if ok`, `if !ok`, or nothing when either arm
    /// is empty.
    fn write_receive_arms(
        &self,
        output: &mut String,
        ok_var: &str,
        some: Option<&str>,
        none: Option<&str>,
    ) {
        match (some, none) {
            (Some(some), Some(none)) => {
                write_line!(output, "if {} {{", ok_var);
                output.push_str(some);
                output.push_str("} else {\n");
                output.push_str(none);
                output.push_str("}\n");
            }
            (Some(some), None) => {
                write_line!(output, "if {} {{", ok_var);
                output.push_str(some);
                output.push_str("}\n");
            }
            (None, Some(none)) => {
                write_line!(output, "if !{} {{", ok_var);
                output.push_str(none);
                output.push_str("}\n");
            }
            (None, None) => {}
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
}

/// Cancel deref-of-address: `*&x` → `x`, `*(&x)` → `x`.
/// When the emitter adds `*` to dereference a ref-typed expression that was
/// already emitted with an `&` prefix, the two operations cancel out.
fn cancel_deref_of_address(ch: String) -> String {
    if let Some(inner) = ch.strip_prefix("(&").and_then(|s| s.strip_suffix(')')) {
        inner.to_string()
    } else if let Some(inner) = ch.strip_prefix('&') {
        inner.to_string()
    } else {
        format!("*{}", ch)
    }
}
