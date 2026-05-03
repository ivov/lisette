use crate::checker::EnvResolve;
use crate::store::Store;
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm, SelectArmPattern, Span};
use syntax::types::{Type, unqualified_name};

use super::super::TaskState;
use crate::validators::temp_producing::is_temp_producing;

impl TaskState<'_> {
    pub(super) fn infer_select(
        &mut self,
        store: &mut Store,
        arms: Vec<SelectArm>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if arms.is_empty() {
            self.sink.push(diagnostics::infer::empty_select(span));
            self.unify(store, expected_ty, &Type::unit(), &span);
            return Expression::Select {
                arms: vec![],
                ty: expected_ty.resolve_in(&self.env),
                span,
            };
        }

        self.check_multiple_select_receives(&arms);
        self.check_duplicate_select_defaults(&arms);

        let result_ty = self.new_type_var();

        let new_arms: Vec<SelectArm> = arms
            .into_iter()
            .map(|arm| {
                self.scopes.push();

                let new_arm_pattern = match arm.pattern {
                    SelectArmPattern::Receive {
                        binding,
                        receive_expression,
                        body,
                        ..
                    } => self.infer_select_receive(
                        store,
                        binding,
                        receive_expression,
                        body,
                        &result_ty,
                    ),

                    SelectArmPattern::Send {
                        send_expression,
                        body,
                    } => self.infer_select_send(store, send_expression, body, &result_ty),

                    SelectArmPattern::MatchReceive {
                        receive_expression,
                        arms: match_arms,
                    } => self.infer_select_match_receive(
                        store,
                        receive_expression,
                        match_arms,
                        &result_ty,
                    ),

                    SelectArmPattern::WildCard { body } => {
                        self.infer_select_wildcard(store, body, &result_ty)
                    }
                };

                self.scopes.pop();

                SelectArm {
                    pattern: new_arm_pattern,
                }
            })
            .collect();

        self.unify(store, expected_ty, &result_ty, &span);

        // Reject non-exhaustive select expressions in value position:
        // shorthand receive arms without a default arm can silently return
        // a zero value when the channel is closed or the pattern doesn't match.
        let resolved_result = result_ty.resolve_in(&self.env);
        if !resolved_result.is_unit() && !resolved_result.is_variable() {
            let has_shorthand_receive = new_arms
                .iter()
                .any(|arm| matches!(arm.pattern, SelectArmPattern::Receive { .. }));
            let has_default = new_arms
                .iter()
                .any(|arm| matches!(arm.pattern, SelectArmPattern::WildCard { .. }));
            if has_shorthand_receive && !has_default {
                self.sink
                    .push(diagnostics::infer::non_exhaustive_select_expression(span));
            }
        }

        Expression::Select {
            arms: new_arms,
            ty: result_ty,
            span,
        }
    }

    fn infer_select_receive(
        &mut self,
        store: &mut Store,
        binding: Box<Pattern>,
        receive_expression: Box<Expression>,
        body: Box<Expression>,
        result_ty: &Type,
    ) -> SelectArmPattern {
        let receive_ty = self.new_type_var();
        let new_receive_expression = self.infer_expression(store, *receive_expression, &receive_ty);

        self.check_complex_select_expression(&new_receive_expression);

        let element_ty = if self.is_channel_receive_call(&new_receive_expression) {
            receive_ty.clone()
        } else {
            self.sink.push(diagnostics::infer::expected_channel_receive(
                &receive_ty,
                new_receive_expression.get_span(),
            ));
            Type::Error
        };

        let inner_binding: &Pattern = match binding.as_ref() {
            Pattern::AsBinding { pattern, span, .. } => {
                let is_some = matches!(pattern.as_ref(), Pattern::EnumVariant { identifier, .. }
                    if unqualified_name(identifier) == "Some");
                if is_some {
                    self.sink
                        .push(diagnostics::infer::select_some_as_binding_not_supported(
                            *span,
                        ));
                } else {
                    self.sink
                        .push(diagnostics::infer::as_binding_in_irrefutable_context(*span));
                }
                pattern.as_ref()
            }
            p => p,
        };

        if matches!(inner_binding, Pattern::Identifier { .. }) {
            self.sink
                .push(diagnostics::infer::bare_identifier_in_select_receive(
                    &binding.get_span(),
                ));
        }

        if let Pattern::EnumVariant {
            identifier, fields, ..
        } = inner_binding
        {
            let variant_name = unqualified_name(identifier);
            if variant_name == "None" {
                self.sink
                    .push(diagnostics::infer::none_pattern_in_select_receive(
                        &binding.get_span(),
                    ));
            }

            if variant_name == "Some"
                && fields.len() == 1
                && !TaskState::is_irrefutable_select_pattern(&fields[0])
            {
                self.sink
                    .push(diagnostics::infer::select_receive_refutable_pattern(
                        fields[0].get_span(),
                    ));
            }
        }

        let (new_binding, typed_pattern) = self.infer_pattern(
            store,
            *binding,
            element_ty.clone(),
            syntax::ast::BindingKind::Let { mutable: false },
        );

        let saved_in_match_arm = self.scopes.set_in_match_arm(true);
        self.scopes.set_in_subexpression(false);
        let new_body = self.infer_expression(store, *body, result_ty);
        self.scopes.set_in_match_arm(saved_in_match_arm);

        SelectArmPattern::Receive {
            binding: Box::new(new_binding),
            typed_pattern: Some(typed_pattern),
            receive_expression: Box::new(new_receive_expression),
            body: Box::new(new_body),
        }
    }

    fn infer_select_send(
        &mut self,
        store: &mut Store,
        send_expression: Box<Expression>,
        body: Box<Expression>,
        result_ty: &Type,
    ) -> SelectArmPattern {
        let send_ty = self.new_type_var();
        let new_send_expression = self.infer_expression(store, *send_expression, &send_ty);

        self.check_complex_select_expression(&new_send_expression);

        if !self.is_channel_send_call(&new_send_expression)
            && !self.is_channel_receive_call(&new_send_expression)
        {
            self.sink.push(diagnostics::infer::expected_channel_send(
                new_send_expression.get_span(),
            ));
        }

        let saved_in_match_arm = self.scopes.set_in_match_arm(true);
        self.scopes.set_in_subexpression(false);
        let new_body = self.infer_expression(store, *body, result_ty);
        self.scopes.set_in_match_arm(saved_in_match_arm);

        SelectArmPattern::Send {
            send_expression: Box::new(new_send_expression),
            body: Box::new(new_body),
        }
    }

    fn infer_select_match_receive(
        &mut self,
        store: &mut Store,
        receive_expression: Box<Expression>,
        match_arms: Vec<MatchArm>,
        result_ty: &Type,
    ) -> SelectArmPattern {
        let receive_ty = self.new_type_var();
        let new_receive_expression = self.infer_expression(store, *receive_expression, &receive_ty);

        self.check_complex_select_expression(&new_receive_expression);

        if !self.is_channel_receive_call(&new_receive_expression) {
            self.sink.push(diagnostics::infer::expected_channel_receive(
                &receive_ty,
                new_receive_expression.get_span(),
            ));
        }

        self.check_select_match_arms(&match_arms, new_receive_expression.get_span());

        let pattern_ty = receive_ty.resolve_in(&self.env);

        let new_match_arms = match_arms
            .into_iter()
            .map(|match_arm| {
                self.scopes.push();

                let (new_pattern, typed_pattern) = self.infer_pattern(
                    store,
                    match_arm.pattern,
                    pattern_ty.clone(),
                    syntax::ast::BindingKind::MatchArm,
                );

                let bool_ty = self.type_bool();
                let new_guard = match_arm.guard.map(|guard| {
                    let guard_expression = self.infer_expression(store, *guard, &bool_ty);
                    Box::new(guard_expression)
                });

                let saved_in_match_arm = self.scopes.set_in_match_arm(true);
                let new_expression = self.infer_expression(store, *match_arm.expression, result_ty);
                self.scopes.set_in_match_arm(saved_in_match_arm);

                self.scopes.pop();

                MatchArm {
                    pattern: new_pattern,
                    guard: new_guard,
                    typed_pattern: Some(typed_pattern),
                    expression: Box::new(new_expression),
                }
            })
            .collect();

        SelectArmPattern::MatchReceive {
            receive_expression: Box::new(new_receive_expression),
            arms: new_match_arms,
        }
    }

    fn infer_select_wildcard(
        &mut self,
        store: &mut Store,
        body: Box<Expression>,
        result_ty: &Type,
    ) -> SelectArmPattern {
        let saved_in_match_arm = self.scopes.set_in_match_arm(true);
        self.scopes.set_in_subexpression(false);
        let new_body = self.infer_expression(store, *body, result_ty);
        self.scopes.set_in_match_arm(saved_in_match_arm);
        SelectArmPattern::WildCard {
            body: Box::new(new_body),
        }
    }

    pub(crate) fn is_channel_receive_call(&self, expression: &Expression) -> bool {
        if let Expression::Call {
            expression, args, ..
        } = expression
        {
            // Dot form: ch.receive()
            if args.is_empty()
                && let Expression::DotAccess {
                    member,
                    expression: receiver,
                    ..
                } = expression.as_ref()
                && member == "receive"
            {
                return self.is_channel_type(&receiver.get_type());
            }
            // UFCS form after inference: Channel.receive(ch) is rewritten to
            // Identifier("Channel.receive") with 1 arg
            if args.len() == 1
                && let Expression::Identifier { value, .. } = expression.as_ref()
                && value.ends_with(".receive")
                && Self::is_ufcs_channel_prefix(value)
                && self.is_channel_type(&args[0].get_type())
            {
                return true;
            }
        }
        false
    }

    /// Check if an expression is a channel send call: `ch.send(value)` or `Channel.send(ch, value)`.
    pub(crate) fn is_channel_send_call(&self, expression: &Expression) -> bool {
        if let Expression::Call {
            expression, args, ..
        } = expression
        {
            // Dot form: ch.send(v) — 1 arg, receiver is channel
            if let Expression::DotAccess {
                member,
                expression: receiver,
                ..
            } = expression.as_ref()
                && member == "send"
                && args.len() == 1
                && self.is_channel_type(&receiver.get_type())
            {
                return true;
            }
            // UFCS form after inference: Channel.send(ch, v) is rewritten to
            // Identifier("Channel.send") with 2 args
            if args.len() == 2
                && let Expression::Identifier { value, .. } = expression.as_ref()
                && value.ends_with(".send")
                && Self::is_ufcs_channel_prefix(value)
                && self.is_channel_type(&args[0].get_type())
            {
                return true;
            }
        }
        false
    }

    /// Check if a UFCS identifier like "Channel.send" or "module.Sender.receive"
    /// has a native channel type as the component immediately before the method name.
    fn is_ufcs_channel_prefix(identifier: &str) -> bool {
        if let Some((prefix, _method)) = identifier.rsplit_once('.') {
            let base = prefix.rsplit_once('.').map(|(_, b)| b).unwrap_or(prefix);
            matches!(base, "Channel" | "Sender" | "Receiver")
        } else {
            false
        }
    }

    /// Check if a type is a channel-like type (Channel, Sender, Receiver).
    fn is_channel_type(&self, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env).strip_refs();
        matches!(resolved.get_name(), Some("Channel" | "Sender" | "Receiver"))
    }

    /// Extract the channel sub-expression from a channel operation call.
    /// Returns the channel expression from `ch.receive()`, `ch.send(v)`,
    /// or UFCS forms like `Channel.receive(ch)`, `Channel.send(ch, v)`.
    fn extract_channel_expression(expression: &Expression) -> Option<&Expression> {
        let Expression::Call {
            expression, args, ..
        } = expression
        else {
            return None;
        };

        // Dot form: ch.send(v) / ch.receive()
        if let Expression::DotAccess {
            expression: channel,
            member,
            ..
        } = expression.as_ref()
            && (member == "send" || member == "receive")
        {
            return Some(channel);
        }

        // UFCS form: Channel.send(ch, v) / Channel.receive(ch)
        if let Expression::Identifier { value, .. } = expression.as_ref()
            && (value.ends_with(".send") || value.ends_with(".receive"))
            && !args.is_empty()
        {
            return Some(&args[0]);
        }

        None
    }

    /// Extract the send value from a channel send call.
    fn extract_send_value(expression: &Expression) -> Option<&Expression> {
        let Expression::Call {
            expression, args, ..
        } = expression
        else {
            return None;
        };

        // Dot form: ch.send(v)
        if let Expression::DotAccess { member, .. } = expression.as_ref()
            && member == "send"
            && args.len() == 1
        {
            return Some(&args[0]);
        }

        // UFCS form: Channel.send(ch, v)
        if let Expression::Identifier { value, .. } = expression.as_ref()
            && value.ends_with(".send")
            && args.len() == 2
        {
            return Some(&args[1]);
        }

        None
    }

    fn check_complex_select_expression(&mut self, expression: &Expression) {
        if let Some(channel) = Self::extract_channel_expression(expression)
            && is_temp_producing(channel)
        {
            self.sink
                .push(diagnostics::infer::complex_select_expression(
                    channel.get_span(),
                ));
        }
        if let Some(value) = Self::extract_send_value(expression)
            && is_temp_producing(value)
        {
            self.sink
                .push(diagnostics::infer::complex_select_expression(
                    value.get_span(),
                ));
        }
    }
}
