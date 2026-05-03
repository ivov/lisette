use crate::Emitter;
use crate::names::go_name;
use crate::patterns::decision_tree;
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
}

struct SelectPrep {
    send_parts: Vec<Option<SendArmParts>>,
    channel_operands: Vec<Option<String>>,
    channel_shadows: Vec<Option<String>>,
}

impl Emitter<'_> {
    pub(crate) fn emit_select(&mut self, output: &mut String, arms: &[SelectArm]) {
        let needs_retry_loop = arms.iter().any(|arm| {
            matches!(&arm.pattern, SelectArmPattern::Receive { binding, .. } if Self::is_some_pattern(binding))
        });

        let prep = self.preprocess_select_arms(output, arms, needs_retry_loop);

        if needs_retry_loop {
            output.push_str("for {\n");
        }

        self.enter_scope();
        output.push_str("select {\n");

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
                    body,
                    ..
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
                    };
                    self.emit_receive_arm(output, binding, typed_pattern.as_ref(), &receiver_ctx);
                }
                SelectArmPattern::Send { body, .. } => {
                    let parts = prep.send_parts[i].as_ref().unwrap();
                    self.emit_send_arm_case(output, parts, body);
                }
                SelectArmPattern::MatchReceive {
                    arms: match_arms, ..
                } => {
                    let channel = prep.channel_operands[i].as_ref().unwrap();
                    self.emit_match_receive_arm(output, match_arms, channel);
                }
                SelectArmPattern::WildCard { body } => {
                    output.push_str("default:\n");
                    self.emit_in_position(output, body);
                }
            }
        }

        output.push_str("}\n");
        self.exit_scope();

        if needs_retry_loop {
            output.push_str("break\n}\n");
            // Go can't see that `break` is unreachable (all select paths either
            // return or continue), so emit panic to satisfy the compiler.
            if self.position.is_tail() {
                output.push_str("panic(\"unreachable\")\n");
            }
        } else {
            let has_default = arms
                .iter()
                .any(|arm| matches!(arm.pattern, SelectArmPattern::WildCard { .. }));
            self.emit_unreachable_if_needed(output, has_default);
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
                    if Self::is_some_pattern(binding) && needs_retry_loop {
                        let shadow = self.fresh_var(Some("ch"));
                        write_line!(output, "{} := {}", shadow, ch);
                        channel_operands.push(Some(ch));
                        channel_shadows.push(Some(shadow));
                    } else {
                        let ch = if needs_retry_loop && channel_has_call {
                            let tmp = self.fresh_var(Some("ch"));
                            write_line!(output, "{} := {}", tmp, ch);
                            tmp
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
                        let tmp = self.fresh_var(Some("ch"));
                        write_line!(output, "{} := {}", tmp, ch);
                        tmp
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
            let ch = self.emit_value(output, channel);
            return if channel.get_type().is_ref() {
                cancel_deref_of_address(ch)
            } else {
                ch
            };
        }
        self.emit_value(output, receive_expression)
    }

    fn fresh_ok_var(&mut self) -> String {
        if self.scope.bindings.has_go_name("ok") || self.is_declared("ok") {
            self.fresh_var(Some("ok"))
        } else {
            "ok".to_string()
        }
    }

    fn emit_ok_check(&mut self, output: &mut String, ok_var: &str, ctx: &SelectReceiveContext) {
        let pre = output.len();
        self.emit_in_position(output, ctx.body);
        let body_empty = output.len() == pre;

        let has_else = ctx.retry_var.is_some() || ctx.default_body.is_some();

        if body_empty && has_else {
            write_line!(output, "if !{} {{", ok_var);
            self.emit_ok_else(output, ctx);
            output.push_str("}\n");
        } else if body_empty {
            // Both branches empty, omit if/else entirely
        } else {
            let body_content = output[pre..].to_string();
            output.truncate(pre);
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
            self.emit_in_position(output, default_body);
        }
    }

    /// Emit the ok-check guard pattern for channel receives with Option semantics.
    /// Produces: `case {receiver_var}, {ok_var} := <-{channel}: if {ok_var} { ... } else { ... }`
    ///
    /// When `inner_pattern` is provided, uses `collect_pattern_info` to emit both
    /// runtime checks (literals, enum tags) and bindings, not just bindings.
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
            let (checks, bindings) = decision_tree::collect_pattern_info(self, pattern, typed);
            if checks.is_empty() {
                decision_tree::emit_tree_bindings(self, output, &bindings, receiver_var);
                self.emit_in_position(output, ctx.body);
            } else {
                let condition = decision_tree::render_condition(&checks, receiver_var);
                write_line!(output, "if {} {{", condition);
                decision_tree::emit_tree_bindings(self, output, &bindings, receiver_var);
                self.emit_in_position(output, ctx.body);
                if let Some(default_body) = ctx.default_body {
                    output.push_str("} else {\n");
                    self.emit_in_position(output, default_body);
                }
                output.push_str("}\n");
            }
        } else {
            self.emit_in_position(output, ctx.body);
        }
        guard.finish(output);
        self.scope.bindings.restore();
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
        let effective_pattern = Self::unwrap_some_pattern(binding);
        let needs_ok_check = Self::is_some_pattern(binding);
        let inner_typed = Self::unwrap_some_typed_pattern(typed_pattern);

        self.scope.bindings.save();

        match effective_pattern {
            Pattern::Identifier { identifier, .. } => {
                if let Some(go_name) = self.go_name_for_binding(effective_pattern) {
                    let var = self.scope.bindings.add(identifier, go_name);
                    if needs_ok_check {
                        self.emit_ok_guard(output, &var, None, ctx);
                        return;
                    } else {
                        write_line!(output, "case {} := <-{}:", var, ctx.channel);
                    }
                } else if needs_ok_check {
                    let ok_var = self.fresh_ok_var();
                    write_line!(output, "case _, {} := <-{}:", ok_var, ctx.channel);
                    self.emit_ok_check(output, &ok_var, ctx);
                    self.scope.bindings.restore();
                    return;
                } else {
                    write_line!(output, "case <-{}:", ctx.channel);
                }
            }
            Pattern::WildCard { .. } => {
                if needs_ok_check {
                    let ok_var = self.fresh_ok_var();
                    write_line!(output, "case _, {} := <-{}:", ok_var, ctx.channel);
                    self.emit_ok_check(output, &ok_var, ctx);
                    self.scope.bindings.restore();
                    return;
                }
                write_line!(output, "case <-{}:", ctx.channel);
            }
            _ => {
                let receiver_var = self.fresh_var(Some("recv"));
                if needs_ok_check {
                    self.emit_ok_guard(
                        output,
                        &receiver_var,
                        Some((effective_pattern, inner_typed)),
                        ctx,
                    );
                    return;
                } else {
                    write_line!(output, "case {} := <-{}:", receiver_var, ctx.channel);
                    self.emit_pattern_bindings(
                        output,
                        &receiver_var,
                        effective_pattern,
                        inner_typed,
                    );
                }
            }
        }
        self.emit_in_position(output, ctx.body);
        self.scope.bindings.restore();
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
            let mut ch = self.emit_value(output, channel);
            if channel.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if ch_has_call {
                let tmp = self.fresh_var(Some("ch"));
                write_line!(output, "{} := {}", tmp, ch);
                ch = tmp;
            }
            match member {
                "send" if !args.is_empty() => {
                    let val_has_call = needs_hoist && contains_call(&args[0]);
                    let mut val = self.emit_composite_value(output, &args[0]);
                    if val_has_call {
                        let tmp = self.fresh_var(Some("send_val"));
                        write_line!(output, "{} := {}", tmp, val);
                        val = tmp;
                    }
                    SendArmParts::Send(ch, val)
                }
                "receive" if args.is_empty() => SendArmParts::Receive(ch),
                _ => SendArmParts::Default,
            }
        } else {
            let expression_has_call = needs_hoist && contains_call(send_expression);
            let mut ch = self.emit_value(output, send_expression);
            if send_expression.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if expression_has_call {
                let tmp = self.fresh_var(Some("ch"));
                write_line!(output, "{} := {}", tmp, ch);
                ch = tmp;
            }
            SendArmParts::Receive(ch)
        }
    }

    /// Emit the `case` line and body for a pre-processed send arm.
    fn emit_send_arm_case(&mut self, output: &mut String, parts: &SendArmParts, body: &Expression) {
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
        self.emit_in_position(output, body);
    }

    fn emit_match_receive_arm(
        &mut self,
        output: &mut String,
        match_arms: &[MatchArm],
        channel: &str,
    ) {
        self.scope.bindings.save();

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
        );
        let none_content = self.capture_scoped(output, |this, output| {
            Emitter::emit_none_arm_body(this, output, match_arms);
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

        self.scope.bindings.restore();
    }

    /// Map a `Some(pattern)` payload pattern to a case-variable name and a
    /// flag indicating whether the payload needs decision-tree destructuring
    /// inside the arm body (as opposed to being bound directly by the
    /// receive-case header).
    fn classify_receive_var_pattern(&mut self, pattern: &Pattern) -> (String, bool) {
        match pattern {
            Pattern::WildCard { .. } => ("_".to_string(), false),
            Pattern::Identifier { identifier, .. } => {
                let Some(go_name) = self.go_name_for_binding(pattern) else {
                    return ("_".to_string(), false);
                };
                if self.scope.bindings.get(identifier).is_some() {
                    return (self.fresh_var(Some("recv")), true);
                }
                (self.scope.bindings.add(identifier, go_name), false)
            }
            _ => (self.fresh_var(Some("recv")), true),
        }
    }

    /// Render the Some arm's body (including payload destructure when
    /// needed), returning the captured content so the caller can wrap it in
    /// an `if ok` guard alongside the None arm.
    fn render_receive_some_arm(
        &mut self,
        output: &mut String,
        some_arm: &MatchArm,
        match_arms: &[MatchArm],
        receiver_var_pattern: &Pattern,
        case_var: &str,
        needs_receiver_destructure: bool,
    ) -> Option<String> {
        self.capture_scoped(output, |this, output| {
            if !needs_receiver_destructure {
                this.emit_in_position(output, &some_arm.expression);
                return;
            }
            let inner_typed = Self::unwrap_some_typed_pattern(some_arm.typed_pattern.as_ref());
            let (checks, bindings) =
                decision_tree::collect_pattern_info(this, receiver_var_pattern, inner_typed);
            if checks.is_empty() {
                decision_tree::emit_tree_bindings(this, output, &bindings, case_var);
                this.emit_in_position(output, &some_arm.expression);
                return;
            }
            let condition = decision_tree::render_condition(&checks, case_var);
            write_line!(output, "if {} {{", condition);
            decision_tree::emit_tree_bindings(this, output, &bindings, case_var);
            this.emit_in_position(output, &some_arm.expression);
            output.push_str("} else {\n");
            Emitter::emit_none_arm_body(this, output, match_arms);
            output.push_str("}\n");
        })
    }

    /// Emit into a scoped buffer, returning the appended content (or `None`
    /// if nothing was written). Used when the combine step needs to know
    /// whether each arm produced any output before emitting the `if ok { ... }`
    /// scaffolding around it.
    fn capture_scoped<F>(&mut self, output: &mut String, f: F) -> Option<String>
    where
        F: FnOnce(&mut Self, &mut String),
    {
        let before = output.len();
        self.enter_scope();
        f(self, output);
        self.exit_scope();
        if output.len() > before {
            let s = output[before..].to_string();
            output.truncate(before);
            Some(s)
        } else {
            None
        }
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

    fn emit_none_arm_body(emitter: &mut Emitter, output: &mut String, match_arms: &[MatchArm]) {
        for match_arm in match_arms {
            if let Pattern::EnumVariant { identifier, .. } = &match_arm.pattern {
                let variant_name = go_name::unqualified_name(identifier);
                if variant_name == "None" {
                    emitter.emit_in_position(output, &match_arm.expression);
                    return;
                }
            }
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

    fn peel_as_binding(pattern: &Pattern) -> &Pattern {
        match pattern {
            Pattern::AsBinding { pattern, .. } => pattern.as_ref(),
            p => p,
        }
    }

    fn unwrap_some_pattern(pattern: &Pattern) -> &Pattern {
        let pattern = Self::peel_as_binding(pattern);
        if let Pattern::EnumVariant {
            identifier, fields, ..
        } = pattern
        {
            let variant_name = go_name::unqualified_name(identifier);
            if variant_name == "Some" && fields.len() == 1 {
                return &fields[0];
            }
        }
        pattern
    }

    fn is_some_pattern(pattern: &Pattern) -> bool {
        let pattern = Self::peel_as_binding(pattern);
        if let Pattern::EnumVariant {
            identifier, fields, ..
        } = pattern
        {
            let variant_name = go_name::unqualified_name(identifier);
            return variant_name == "Some" && fields.len() == 1;
        }
        false
    }

    fn unwrap_some_typed_pattern(typed: Option<&TypedPattern>) -> Option<&TypedPattern> {
        if let Some(TypedPattern::EnumVariant {
            variant_name,
            fields,
            ..
        }) = typed
            && variant_name == "Some"
            && fields.len() == 1
        {
            return Some(&fields[0]);
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
