use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::patterns::sites::{
    self, AnnotatedPattern, PatternSubject, TypedSubject, unwrap_some_pattern,
    unwrap_some_typed_pattern,
};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, SelectArmPlan, SelectStatementPlan,
};
use crate::plan::placement::unreachable_panic_if_needed;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm, SelectArmPattern, TypedPattern};
use syntax::program::{ChannelOperation, channel_operation};
use syntax::types::Type;

enum PreparedChannelOperation {
    Send(String, String),
    Receive(String),
}

struct SelectReceiveContext<'a> {
    channel: &'a str,
    body: &'a Expression,
    default_body: Option<&'a Expression>,
    retry_var: Option<&'a str>,
    element_ty: syntax::types::Type,
    place: &'a PlacePlan<'a>,
}

enum PreparedSelectArm<'a> {
    Receive {
        binding: &'a Pattern,
        typed_pattern: Option<&'a TypedPattern>,
        body: &'a Expression,
        channel: String,
        retry_on_close: bool,
        element_ty: Type,
    },
    Send {
        body: &'a Expression,
        operation: PreparedChannelOperation,
    },
    MatchReceive {
        arms: &'a [MatchArm],
        channel: String,
        element_ty: Type,
    },
    Default {
        body: &'a Expression,
    },
}

impl Planner<'_> {
    /// Lower a `select` expression to a structured `SelectStatementPlan`.
    pub(crate) fn lower_select(
        &mut self,
        arms: &[SelectArm],
        place: &PlacePlan,
    ) -> SelectStatementPlan {
        let needs_retry_loop = arms.iter().any(|arm| {
            matches!(&arm.pattern, SelectArmPattern::Receive { binding, .. } if binding.is_some_pattern())
        });

        let mut setup: Vec<LoweredStatement> = Vec::new();
        let prep = self.preprocess_select_arms(&mut setup, arms, needs_retry_loop);

        let has_default = prep
            .iter()
            .any(|arm| matches!(arm, PreparedSelectArm::Default { .. }));

        self.enter_scope();
        let arm_plans = self.lower_select_arms(prep, place);
        self.exit_scope();

        let all_arms_diverge =
            !arm_plans.is_empty() && arm_plans.iter().all(|arm| arm.body().ends_with_diverge());
        let exhaustive = all_arms_diverge || if needs_retry_loop { false } else { has_default };
        let mut postlude: Vec<LoweredStatement> = Vec::new();
        if let Some(panic) = unreachable_panic_if_needed(place, exhaustive) {
            postlude.push(panic);
        }

        SelectStatementPlan {
            setup,
            retry_loop: needs_retry_loop,
            arms: arm_plans,
            postlude,
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
                    typed_pattern,
                    body,
                    channel,
                    retry_on_close,
                    element_ty,
                } => {
                    let receiver_ctx = SelectReceiveContext {
                        channel: &channel,
                        body,
                        default_body,
                        retry_var: retry_on_close.then_some(channel.as_str()),
                        element_ty,
                        place,
                    };
                    self.lower_receive_arm(binding, typed_pattern, &receiver_ctx)
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
        setup: &mut Vec<LoweredStatement>,
        arms: &'a [SelectArm],
        needs_retry_loop: bool,
    ) -> Vec<PreparedSelectArm<'a>> {
        let mut prepared = Vec::with_capacity(arms.len());

        for arm in arms {
            let prepared_arm = match &arm.pattern {
                SelectArmPattern::Send {
                    send_expression,
                    body,
                } => PreparedSelectArm::Send {
                    body,
                    operation: self.prepare_send_arm(setup, send_expression, needs_retry_loop),
                },
                SelectArmPattern::Receive {
                    receive_expression,
                    binding,
                    typed_pattern,
                    body,
                    ..
                } => {
                    let channel = self.lower_channel_operand(receive_expression);
                    let channel_has_call = channel.evaluation.effect.has_call();
                    let (channel_setup, ch) = channel.into_parts();
                    setup.extend(channel_setup);
                    let (channel, retry_on_close) = if binding.is_some_pattern() && needs_retry_loop
                    {
                        (self.hoist_tmp_value_statement(setup, "ch", &ch), true)
                    } else {
                        let ch = if needs_retry_loop && channel_has_call {
                            self.hoist_tmp_value_statement(setup, "ch", &ch)
                        } else {
                            ch
                        };
                        (ch, false)
                    };
                    PreparedSelectArm::Receive {
                        binding,
                        typed_pattern: typed_pattern.as_ref(),
                        body,
                        channel,
                        retry_on_close,
                        element_ty: receive_expression.get_type().ok_type(),
                    }
                }
                SelectArmPattern::MatchReceive {
                    receive_expression,
                    arms,
                } => {
                    let channel = self.lower_channel_operand(receive_expression);
                    let channel_has_call = channel.evaluation.effect.has_call();
                    let (channel_setup, ch) = channel.into_parts();
                    setup.extend(channel_setup);
                    let ch = if needs_retry_loop && channel_has_call {
                        self.hoist_tmp_value_statement(setup, "ch", &ch)
                    } else {
                        ch
                    };
                    PreparedSelectArm::MatchReceive {
                        arms,
                        channel: ch,
                        element_ty: receive_expression.get_type().ok_type(),
                    }
                }
                SelectArmPattern::WildCard { body } => PreparedSelectArm::Default { body },
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
                return plan.map_rendered(|_, value, contains_deferred_evaluation| {
                    GoExpression::opaque_with_deferred_evaluation(
                        cancel_deref_of_address(value),
                        contains_deferred_evaluation,
                    )
                });
            }
            return plan;
        }
        self.lower_value(receive_expression, ExpressionContext::value())
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
    ) -> Vec<LoweredStatement> {
        // Decide scaffolding on rendered emptiness, not `is_empty`: some lowered
        // statements (e.g. a discard `let _`) render to empty text even when the
        // IR is structurally non-empty.
        let body_block = self.lower_block_to_place(ctx.body, ctx.place);
        let body_empty = Renderer.renders_empty(&body_block);
        let else_block = self.build_ok_else_block(ctx);
        let has_else = else_block.is_some();

        if body_empty && !has_else {
            return Vec::new();
        }

        let plan = if body_empty {
            IfPlan {
                condition_setup: Vec::new(),
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
                condition_setup: Vec::new(),
                condition: ok_var.to_string(),
                then_body: body_block,
                else_arm,
            }
        };
        vec![LoweredStatement::If(plan)]
    }

    /// Else branch for an ok-check: retry (`v = nil; continue`) or default
    /// body. `None` when neither applies or the default lowers empty.
    fn build_ok_else_block(&mut self, ctx: &SelectReceiveContext) -> Option<LoweredBlock> {
        if let Some(retry_var) = ctx.retry_var {
            return Some(LoweredBlock {
                statements: vec![
                    LoweredStatement::RawGo(format!("{} = nil\n", retry_var)),
                    LoweredStatement::Continue {
                        target: None,
                        label: None,
                    },
                ],
            });
        }
        let default_body = ctx.default_body?;
        let block = self.lower_block_to_place(default_body, ctx.place);
        (!block.is_empty()).then_some(block)
    }

    /// `case v, ok := <-ch:` plus an `if ok { ... } else { ... }` body.
    fn lower_ok_guard(
        &mut self,
        receiver_var: &str,
        inner_pattern: Option<(&Pattern, Option<&TypedPattern>)>,
        ctx: &SelectReceiveContext,
    ) -> SelectArmPlan {
        let ok_var = self.fresh_ok_var();
        let receive_vars = format!("{}, {}", receiver_var, ok_var);
        self.scope.enter_use_region();
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
            )
        } else {
            self.lower_block_to_place(ctx.body, ctx.place).statements
        };
        let used = self.scope.exit_use_region();
        let body_holder = LoweredBlock {
            statements: body_statements,
        };
        let mut then_statements: Vec<LoweredStatement> = Vec::new();
        let references_receiver = used.contains(receiver_var);
        if !references_receiver {
            then_statements.push(LoweredStatement::RawGo(format!("_ = {}\n", receiver_var)));
        }
        then_statements.extend(body_holder.statements);
        self.scope.pop_binding_frame();

        let else_arm = match self.build_ok_else_block(ctx) {
            Some(body) => ElseArm::Else {
                body,
                inline: false,
            },
            None => ElseArm::None,
        };
        let if_plan = IfPlan {
            condition_setup: Vec::new(),
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
    ) -> SelectArmPlan {
        let effective_pattern = unwrap_some_pattern(binding);
        let inner_typed = unwrap_some_typed_pattern(typed_pattern);

        self.scope.push_binding_frame();
        if binding.is_some_pattern() {
            self.lower_receive_arm_with_ok_check(effective_pattern, inner_typed, ctx)
        } else {
            self.lower_receive_arm_simple(effective_pattern, inner_typed, ctx)
        }
    }

    /// `case x, ok := <-ch:` with an `if ok` guard or `if !ok { break }`.
    fn lower_receive_arm_with_ok_check(
        &mut self,
        effective_pattern: &Pattern,
        inner_typed: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
    ) -> SelectArmPlan {
        if let Pattern::Identifier { identifier, .. } = effective_pattern
            && let Some(go_name) = self.go_name_for_binding(effective_pattern)
        {
            let var = self.scope.bind(identifier, go_name);
            return self.lower_ok_guard(&var, None, ctx);
        }
        if matches!(
            effective_pattern,
            Pattern::Identifier { .. } | Pattern::WildCard { .. }
        ) {
            let ok_var = self.fresh_ok_var();
            let body = self.lower_ok_check(&ok_var, ctx);
            self.scope.pop_binding_frame();
            return SelectArmPlan::Receive {
                receive_vars: Some(format!("_, {}", ok_var)),
                channel: ctx.channel.to_string(),
                body: LoweredBlock { statements: body },
            };
        }
        let receiver_var = self.fresh_var(Some("recv"));
        self.lower_ok_guard(&receiver_var, Some((effective_pattern, inner_typed)), ctx)
    }

    /// Plain receive: `case v := <-ch:` then the arm body.
    fn lower_receive_arm_simple(
        &mut self,
        effective_pattern: &Pattern,
        inner_typed: Option<&TypedPattern>,
        ctx: &SelectReceiveContext,
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
            body_statements.extend(self.lower_irrefutable_pattern_site(
                PatternSubject::for_value(receiver_var.clone()),
                effective_pattern,
                inner_typed,
                &ctx.element_ty,
            ));
            Some(receiver_var)
        };
        let block = self.lower_block_to_place(ctx.body, ctx.place);
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
        setup: &mut Vec<LoweredStatement>,
        send_expression: &Expression,
        needs_hoist: bool,
    ) -> PreparedChannelOperation {
        let unwrapped = send_expression.unwrap_parens();
        if let Some(operation) = channel_operation(unwrapped) {
            let channel = operation.channel();
            let channel_plan = self.lower_value(channel, ExpressionContext::value());
            let ch_has_call = needs_hoist && channel_plan.evaluation.effect.has_call();
            let (op_setup, mut ch) = channel_plan.into_parts();
            setup.extend(op_setup);
            if channel.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if ch_has_call {
                ch = self.hoist_tmp_value_statement(setup, "ch", &ch);
            }
            match operation {
                ChannelOperation::Send { value, .. } => {
                    let value_plan = self.lower_composite_value(value, ExpressionContext::value());
                    let val_has_call = needs_hoist && value_plan.evaluation.effect.has_call();
                    let (val_setup, mut val) = value_plan.into_parts();
                    setup.extend(val_setup);
                    if val_has_call {
                        val = self.hoist_tmp_value_statement(setup, "send_val", &val);
                    }
                    PreparedChannelOperation::Send(ch, val)
                }
                ChannelOperation::Receive { .. } => PreparedChannelOperation::Receive(ch),
            }
        } else {
            let expression_plan = self.lower_value(send_expression, ExpressionContext::value());
            let expression_has_call = needs_hoist && expression_plan.evaluation.effect.has_call();
            let (op_setup, mut ch) = expression_plan.into_parts();
            setup.extend(op_setup);
            if send_expression.get_type().is_ref() {
                ch = cancel_deref_of_address(ch);
            }
            if expression_has_call {
                ch = self.hoist_tmp_value_statement(setup, "ch", &ch);
            }
            PreparedChannelOperation::Receive(ch)
        }
    }

    /// `case <send>:` (or `default:`) plus the arm body.
    fn lower_send_arm(
        &mut self,
        operation: &PreparedChannelOperation,
        body: &Expression,
        place: &PlacePlan,
    ) -> SelectArmPlan {
        let block = self.lower_block_to_place(body, place);
        match operation {
            PreparedChannelOperation::Send(ch, val) => SelectArmPlan::Send {
                operation: GoExpression::opaque(format!("{} <- {}", ch, val)),
                body: block,
            },
            PreparedChannelOperation::Receive(ch) => SelectArmPlan::Send {
                operation: GoExpression::receive(GoExpression::opaque(ch.clone())),
                body: block,
            },
        }
    }

    fn lower_match_receive_arm(
        &mut self,
        match_arms: &[MatchArm],
        channel: &str,
        element_ty: &syntax::types::Type,
        place: &PlacePlan,
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

        self.scope.enter_use_region();
        let some_block = self.lower_receive_some_arm(
            some_arm,
            match_arms,
            receiver_var_pattern,
            &case_var,
            needs_receiver_destructure,
            element_ty,
            place,
        );
        let none_block =
            self.capture_scoped_block(|this| sites::lower_none_arm_body(this, match_arms, place));

        let arms_plan = build_receive_arms_plan(&ok_var, some_block, none_block);
        if arms_plan.is_some() {
            self.scope.record_go_use(&ok_var);
        }
        let used = self.scope.exit_use_region();

        // Compose the receive-arms body as a structured `if ok { ... } else
        // { ... }` plan so usage of `case_var` and `ok_var` can be checked
        // structurally before deciding whether to emit per-var discards.
        let body_block = LoweredBlock {
            statements: match arms_plan {
                Some(plan) => vec![LoweredStatement::If(plan)],
                None => Vec::new(),
            },
        };

        self.scope.pop_binding_frame();

        // Per-var discards (emitted when the body does not reference the var)
        // precede the structured body inside the `case x, ok := <-ch:` arm.
        let mut body_statements: Vec<LoweredStatement> = Vec::new();
        let references_ok = used.contains(&ok_var);
        if !references_ok {
            body_statements.push(LoweredStatement::RawGo(format!("_ = {}\n", ok_var)));
        }
        let references_case = used.contains(&case_var);
        if case_var != "_" && !references_case {
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
    ) -> Option<LoweredBlock> {
        self.capture_scoped_block(|this| {
            if !needs_receiver_destructure {
                return this.lower_block_to_place(&some_arm.expression, place);
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

/// `if ok { Some } else { None }`, collapsing to `if ok`/`if !ok`/`None`
/// when one or both arms are empty.
fn build_receive_arms_plan(
    ok_var: &str,
    some: Option<LoweredBlock>,
    none: Option<LoweredBlock>,
) -> Option<IfPlan> {
    match (some, none) {
        (Some(some), Some(none)) => Some(IfPlan {
            condition_setup: Vec::new(),
            condition: ok_var.to_string(),
            then_body: some,
            else_arm: ElseArm::Else {
                body: none,
                inline: false,
            },
        }),
        (Some(some), None) => Some(IfPlan {
            condition_setup: Vec::new(),
            condition: ok_var.to_string(),
            then_body: some,
            else_arm: ElseArm::None,
        }),
        (None, Some(none)) => Some(IfPlan {
            condition_setup: Vec::new(),
            condition: format!("!{}", ok_var),
            then_body: none,
            else_arm: ElseArm::None,
        }),
        (None, None) => None,
    }
}
