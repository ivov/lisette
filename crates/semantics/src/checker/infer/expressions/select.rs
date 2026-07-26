use crate::checker::EnvResolve;
use crate::facts::{BranchArm, SelectExhaustivenessCheck};
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm, Span};
use syntax::program::{ChannelOperation, channel_operation};
use syntax::types::{Type, unqualified_name};

use crate::checker::infer::InferCtx;

fn select_arm_body_span(pattern: &SelectArm) -> Span {
    match pattern {
        SelectArm::Receive { body, .. }
        | SelectArm::Send { body, .. }
        | SelectArm::WildCard { body } => body.get_span(),
        SelectArm::MatchReceive {
            receive_expression, ..
        } => receive_expression.get_span(),
    }
}

impl InferCtx<'_> {
    pub fn resolve_select_exhaustiveness(&mut self) {
        for check in std::mem::take(&mut self.facts.select_exhaustiveness_checks) {
            let resolved = check.result_ty.resolve_in(&self.env);
            if !resolved.is_unit() && !resolved.is_variable() {
                self.sink
                    .push(diagnostics::infer::non_exhaustive_select_expression(
                        check.span,
                    ));
            }
        }
    }

    pub(super) fn infer_select(
        &mut self,
        arms: Vec<SelectArm>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if arms.is_empty() {
            self.sink.push(diagnostics::infer::empty_select(span));
            self.unify(expected_ty, &Type::unit(), &span);
            return Expression::Select {
                arms: vec![],
                ty: expected_ty.resolve_in(&self.env),
                span,
            };
        }

        self.check_multiple_select_receives(&arms);
        self.check_duplicate_select_defaults(&arms);

        let result_ty = self.new_type_var();
        self.unify(expected_ty, &result_ty, &span);

        let needs_reconciliation = result_ty.resolve_in(&self.env).is_variable();
        let value_position = needs_reconciliation && !expected_ty.is_ignored();

        let mut branches: Vec<BranchArm> = if needs_reconciliation {
            Vec::with_capacity(arms.len())
        } else {
            Vec::new()
        };

        let new_arms: Vec<SelectArm> = arms
            .into_iter()
            .map(|arm| {
                self.with_scope(|this| {
                    let independent_ty;
                    let arm_target = if needs_reconciliation {
                        independent_ty = this.new_type_var();
                        &independent_ty
                    } else {
                        &result_ty
                    };

                    let new_arm = match arm {
                        SelectArm::Receive {
                            binding,
                            receive_expression,
                            body,
                            ..
                        } => {
                            this.infer_select_receive(binding, receive_expression, body, arm_target)
                        }

                        SelectArm::Send {
                            send_expression,
                            body,
                        } => this.infer_select_send(send_expression, body, arm_target),

                        SelectArm::MatchReceive {
                            receive_expression,
                            arms: match_arms,
                        } => this.infer_select_match_receive(
                            receive_expression,
                            match_arms,
                            arm_target,
                            value_position,
                        ),

                        SelectArm::WildCard { body } => {
                            this.infer_select_wildcard(body, arm_target)
                        }
                    };

                    if needs_reconciliation {
                        branches.push(BranchArm {
                            ty: arm_target.clone(),
                            span: select_arm_body_span(&new_arm),
                        });
                    }

                    new_arm
                })
            })
            .collect();

        if value_position {
            self.reconcile_and_unify(&result_ty, &branches, &span);
        } else if let Some(first) = branches.first() {
            let _ = self.try_unify(&result_ty, &first.ty, &span);
        }

        let shorthand_receive_count = new_arms
            .iter()
            .filter(|arm| matches!(arm, SelectArm::Receive { .. }))
            .count();
        let has_default = new_arms
            .iter()
            .any(|arm| matches!(arm, SelectArm::WildCard { .. }));
        if !expected_ty.is_ignored() && shorthand_receive_count == 1 && !has_default {
            self.facts
                .select_exhaustiveness_checks
                .push(SelectExhaustivenessCheck {
                    result_ty: result_ty.clone(),
                    span,
                });
        }

        Expression::Select {
            arms: new_arms,
            ty: result_ty,
            span,
        }
    }

    fn infer_select_receive(
        &mut self,
        binding: Box<Pattern>,
        receive_expression: Box<Expression>,
        body: Box<Expression>,
        result_ty: &Type,
    ) -> SelectArm {
        let receive_ty = self.new_type_var();
        let new_receive_expression = self.infer_expression(*receive_expression, &receive_ty);

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
                    binding.get_span(),
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
                        binding.get_span(),
                    ));
            }

            if variant_name == "Some"
                && fields.len() == 1
                && !Self::is_irrefutable_select_pattern(&fields[0])
            {
                self.sink
                    .push(diagnostics::infer::select_receive_refutable_pattern(
                        fields[0].get_span(),
                    ));
            }
        }

        let new_binding = self.infer_pattern(
            *binding,
            element_ty.clone(),
            syntax::ast::BindingKind::Let { mutable: false },
        );

        let new_body = self.infer_root_expression(*body, result_ty);

        SelectArm::Receive {
            binding: Box::new(new_binding),
            receive_expression: Box::new(new_receive_expression),
            body: Box::new(new_body),
        }
    }

    fn infer_select_send(
        &mut self,
        send_expression: Box<Expression>,
        body: Box<Expression>,
        result_ty: &Type,
    ) -> SelectArm {
        let send_ty = self.new_type_var();
        let new_send_expression = self.infer_expression(*send_expression, &send_ty);

        self.check_complex_select_expression(&new_send_expression);

        if !self.is_channel_send_call(&new_send_expression)
            && !self.is_channel_receive_call(&new_send_expression)
        {
            self.sink.push(diagnostics::infer::expected_channel_send(
                new_send_expression.get_span(),
            ));
        }

        let new_body = self.infer_root_expression(*body, result_ty);

        SelectArm::Send {
            send_expression: Box::new(new_send_expression),
            body: Box::new(new_body),
        }
    }

    fn infer_select_match_receive(
        &mut self,
        receive_expression: Box<Expression>,
        match_arms: Vec<MatchArm>,
        result_ty: &Type,
        value_position: bool,
    ) -> SelectArm {
        let receive_ty = self.new_type_var();
        let new_receive_expression = self.infer_expression(*receive_expression, &receive_ty);

        self.check_complex_select_expression(&new_receive_expression);

        if !self.is_channel_receive_call(&new_receive_expression) {
            self.sink.push(diagnostics::infer::expected_channel_receive(
                &receive_ty,
                new_receive_expression.get_span(),
            ));
        }

        self.check_select_match_arms(&match_arms, new_receive_expression.get_span());

        let pattern_ty = receive_ty.resolve_in(&self.env);

        let needs_reconciliation = result_ty.resolve_in(&self.env).is_variable();
        let reconcile_in_value_position = needs_reconciliation && value_position;

        let mut branches: Vec<BranchArm> = if needs_reconciliation {
            Vec::with_capacity(match_arms.len())
        } else {
            Vec::new()
        };

        let new_match_arms: Vec<MatchArm> = match_arms
            .into_iter()
            .map(|match_arm| {
                self.with_scope(|this| {
                    let new_pattern = this.infer_pattern(
                        match_arm.pattern,
                        pattern_ty.clone(),
                        syntax::ast::BindingKind::MatchArm,
                    );

                    let bool_ty = this.type_bool();
                    let new_guard = match_arm.guard.map(|guard| {
                        let guard_expression = this.infer_expression(*guard, &bool_ty);
                        Box::new(guard_expression)
                    });

                    let independent_ty;
                    let arm_expected = if needs_reconciliation {
                        independent_ty = this.new_type_var();
                        &independent_ty
                    } else {
                        result_ty
                    };

                    let new_expression =
                        this.infer_root_expression(*match_arm.expression, arm_expected);

                    if needs_reconciliation {
                        branches.push(BranchArm {
                            ty: arm_expected.clone(),
                            span: new_expression.get_span(),
                        });
                    }

                    MatchArm {
                        pattern: new_pattern,
                        guard: new_guard,
                        expression: Box::new(new_expression),
                    }
                })
            })
            .collect();

        let span = new_receive_expression.get_span();
        if reconcile_in_value_position {
            self.reconcile_and_unify(result_ty, &branches, &span);
        } else if let Some(first) = branches.first() {
            let _ = self.try_unify(result_ty, &first.ty, &span);
        }

        SelectArm::MatchReceive {
            receive_expression: Box::new(new_receive_expression),
            arms: new_match_arms,
        }
    }

    fn infer_select_wildcard(&mut self, body: Box<Expression>, result_ty: &Type) -> SelectArm {
        let new_body = self.infer_root_expression(*body, result_ty);
        SelectArm::WildCard {
            body: Box::new(new_body),
        }
    }

    fn is_channel_receive_call(&self, expression: &Expression) -> bool {
        matches!(
            self.valid_channel_operation(expression),
            Some(ChannelOperation::Receive { .. })
        )
    }

    fn is_channel_send_call(&self, expression: &Expression) -> bool {
        matches!(
            self.valid_channel_operation(expression),
            Some(ChannelOperation::Send { .. })
        )
    }

    fn valid_channel_operation<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<ChannelOperation<'a>> {
        let operation = channel_operation(expression)?;
        self.is_channel_type(&operation.channel().get_type())
            .then_some(operation)
    }

    fn is_channel_type(&self, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env).strip_refs();
        matches!(resolved.get_name(), Some("Channel" | "Sender" | "Receiver"))
    }

    fn check_complex_select_expression(&mut self, expression: &Expression) {
        let Some(operation) = channel_operation(expression) else {
            return;
        };
        for operand in [Some(operation.channel()), operation.value()]
            .into_iter()
            .flatten()
        {
            if operand.is_temp_producing() {
                self.sink
                    .push(diagnostics::infer::complex_select_expression(
                        operand.get_span(),
                    ));
            }
        }
    }
}
