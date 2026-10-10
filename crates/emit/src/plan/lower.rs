use crate::Planner;
use crate::abi::transition::try_emit_lowered_tail_return;
use crate::analyze::component_uses::component_demand;
use crate::calls::bound_value::PairCondition;
use crate::calls::predicates::strip_negations;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::control_flow::targets::legalize_source_loop;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    PlacePlan, Statement, directed_first, with_setup,
};
use crate::plan::go_expression::UnaryOp;
use crate::plan::placement::{
    ElidableTail, collapse_declared_temp, requires_temp_var, try_elide_tail_let,
};
use crate::plan::values::{GoExpression, ValuePlan};
use std::iter;
use std::slice;
use syntax::ast::{Expression, IfLetAlternative, Literal, MatchArm, Pattern, Span};
use syntax::types::Type;

fn if_let_match_arms(
    pattern: &Pattern,
    consequence: &Expression,
    alternative: &IfLetAlternative,
    span: Span,
) -> Vec<MatchArm> {
    let alternative = alternative
        .expression()
        .cloned()
        .unwrap_or(Expression::Unit {
            ty: Type::unit(),
            span,
        });
    vec![
        MatchArm {
            pattern: pattern.clone(),
            guard: None,
            expression: Box::new(consequence.clone()),
        },
        MatchArm {
            pattern: Pattern::WildCard {
                span: alternative.get_span(),
            },
            guard: None,
            expression: Box::new(alternative.clone()),
        },
    ]
}

impl Planner<'_> {
    /// Allocate a fresh operand-temp result var and its `var V T` declaration
    /// as a typed setup leaf. The control-flow that assigns it follows as a
    /// typed `If`/`Loop`/`Match`/`Select` statement.
    pub(crate) fn operand_temp_declaration(&mut self, ty: &Type) -> (String, Statement) {
        let result_var = self.fresh_var(None);
        let declaration = LoweredStatement::VarDecl {
            name: result_var.clone().into(),
            go_type: self.use_go_type(ty),
            value: None,
        }
        .into();
        self.declare(&result_var);
        (result_var, declaration)
    }

    /// Lower a value-position `if`/`if let`/`match`/`select` to a fresh
    /// operand-temp variable. Only valid for non-never result types; never-typed
    /// branches route through `lower_to_operand_temp` as a diverging statement.
    pub(crate) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let target = GoExpression::name(result_var.clone());
        let block = self.lower_branching_to_block(
            expression,
            &PlacePlan::Assign {
                local: &target,
                target_ty: Some(ty),
            },
        );
        let mut setup = vec![declaration];
        setup.extend(block.statements);
        collapse_declared_temp(
            &mut setup,
            &result_var,
            self.short_declaration_keeps_type(ty),
        );
        ValuePlan::captured(setup, result_var)
    }

    /// Lower a value-position `loop` to a fresh operand-temp variable.
    /// Declares `var V T`, pushes `V` as the current loop result slot so
    /// `break value` assigns into it, lowers the loop, then renders.
    pub(crate) fn plan_loop_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let Expression::Loop { body, .. } = expression else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let plan = self.with_loop(GoExpression::name(result_var.clone()), |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        });
        ValuePlan::captured(
            vec![declaration, LoweredStatement::Loop(plan).into()],
            result_var,
        )
    }

    fn lower_body_until_diverge(
        &mut self,
        rest: &[Expression],
        last: &Expression,
    ) -> (Vec<Statement>, bool) {
        let mut statements: Vec<Statement> = Vec::with_capacity(rest.len() + 1);
        let forwarded = rest
            .split_last()
            .and_then(|(check, rest)| Some((check, rest, self.forwarded_error_call(check, last)?)));
        let lowered = forwarded.map_or(rest, |(_, rest, _)| rest);
        for (index, item) in lowered.iter().enumerate() {
            let region = rest[index + 1..].iter().chain(iter::once(last));
            let statement = self.lower_block_item(item, region);
            let diverged = statement.kind.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                return (statements, true);
            }
        }
        if let Some((check, _, call)) = forwarded {
            let (mut body, call) = self
                .lower_call(call, None, ExpressionContext::value())
                .into_parts();
            body.push(plain_return(call));
            statements.push(self.directed_at(
                check,
                LoweredStatement::Body(LoweredBlock { statements: body }),
            ));
            return (statements, true);
        }
        statements.extend(self.lower_return_tail(last));
        (statements, false)
    }

    pub(crate) fn lower_function_body(
        &mut self,
        body: &Expression,
        should_return: bool,
    ) -> LoweredBlock {
        if !should_return {
            return self.lower_block_as_body(body);
        }

        let items: &[Expression] = if let Expression::Block { items, .. } = body {
            items
        } else {
            slice::from_ref(body)
        };

        let Some((last, rest)) =
            try_elide_tail_let(items, ElidableTail::FallibleBlock).or_else(|| items.split_last())
        else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        let (mut statements, diverged) = self.lower_body_until_diverge(rest, last);
        if diverged {
            return LoweredBlock { statements };
        }

        // A unit/statement-only body under a non-unit signature has no value to
        // return, so `lower_return_tail` emits it as a bare statement. A
        // function body must still close with an explicit zero-value return;
        // branch arms instead rely on a trailing unreachable panic.
        let is_statement_only = matches!(
            last,
            Expression::Assignment { .. } | Expression::Let { .. } | Expression::Const { .. }
        );
        let is_unit_tail = !is_statement_only
            && !matches!(last, Expression::Return { .. })
            && last.get_type().is_unit();
        let return_ctx = self.return_ctx();
        if (is_statement_only || is_unit_tail)
            && let Some(return_ty) = return_ctx.ty().filter(|ty| !ty.is_unit())
        {
            let return_ty = return_ty.clone();
            let zero = self.zero_value_expression(&return_ty);
            statements.push(plain_return(zero));
        }

        LoweredBlock { statements }
    }

    pub(crate) fn lower_block_item<'r>(
        &mut self,
        item: &Expression,
        region: impl IntoIterator<Item = &'r Expression>,
    ) -> Statement {
        let Expression::Let {
            binding,
            value,
            mode,
            ..
        } = item
        else {
            return self.lower_statement(item);
        };
        let demand = match &binding.pattern {
            Pattern::Identifier {
                binding: Some(id), ..
            } if mode.else_block().is_none() && !binding.is_mutable() => {
                component_demand(region, *id)
            }
            _ => None,
        };
        let plan = self.build_let_plan(binding, value, mode, demand);
        self.directed_at(item, LoweredStatement::Body(plan))
    }

    /// Lower a single statement in the enclosing return context.
    pub(crate) fn lower_statement(&mut self, expression: &Expression) -> Statement {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let statement = self.lower_if(
                    condition,
                    consequence,
                    alternative.as_deref(),
                    &PlacePlan::Statement,
                );
                self.directed_at(expression, statement)
            }
            Expression::Loop { body, .. } => {
                let plan = self.lower_infinite_loop(body);
                self.directed_at(expression, LoweredStatement::Loop(plan))
            }
            Expression::While {
                condition, body, ..
            } => {
                let plan = self.lower_while(condition, body);
                self.directed_at(expression, LoweredStatement::Loop(plan))
            }
            Expression::Block { .. } => LoweredStatement::Block(
                self.with_scope(|this| this.lower_block_as_body(expression)),
            )
            .into(),
            Expression::For { .. } => self.lower_for_statement(expression),
            Expression::Continue { .. } => {
                let target = self
                    .current_loop_id()
                    .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source);
                self.directed_at(expression, LoweredStatement::Continue(target))
            }
            Expression::Break { value: None, .. } => {
                let target = self
                    .current_loop_id()
                    .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source);
                self.directed_at(expression, LoweredStatement::Break(target))
            }
            Expression::Break {
                value: Some(value), ..
            } => {
                let plan = self.build_break_value_plan(value);
                self.directed_at(expression, LoweredStatement::Body(plan))
            }
            Expression::Const {
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let Some(value) = value.value() else {
                    return LoweredStatement::Block(LoweredBlock { statements: vec![] }).into();
                };
                let statement = self.lower_const(identifier, value, ty);
                self.directed_at(expression, statement)
            }
            Expression::Return {
                expression: value, ..
            } => {
                let plan = self.build_return_plan(value);
                self.directed_at(expression, LoweredStatement::Body(plan))
            }
            Expression::Let {
                binding,
                value,
                mode,
                ..
            } => {
                let plan = self.build_let_plan(binding, value, mode, None);
                self.directed_at(expression, LoweredStatement::Body(plan))
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                let statement =
                    self.build_assignment_plan(target, value, compound_operator.as_ref());
                self.directed_at(expression, statement)
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => {
                let arms = if_let_match_arms(pattern, consequence, alternative, *span);
                let body = self.lower_match_to_block(scrutinee, &arms, &PlacePlan::Statement);
                self.directed_at(expression, LoweredStatement::Body(body))
            }
            Expression::Match { subject, arms, .. } => {
                let body = self.lower_match_to_block(subject, arms, &PlacePlan::Statement);
                self.directed_at(expression, LoweredStatement::Body(body))
            }
            Expression::Select { arms, .. } => {
                let statement = self.lower_select(arms, &PlacePlan::Statement);
                self.directed_at(expression, statement)
            }
            Expression::WhileLet { .. } => self.lower_while_let_statement(expression),
            Expression::Assert { .. } => self.lower_assert_statement(expression).into(),
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::ImplBlock { .. } => {
                unreachable!("the parser rejects item definitions inside function bodies")
            }
            Expression::Call { .. } if self.is_test_log_call(expression) => {
                self.lower_test_log_statement(expression).into()
            }
            _ => self.lower_expression_statement(expression),
        }
    }

    fn directed_at(&self, expression: &Expression, kind: LoweredStatement) -> Statement {
        Statement {
            line: self.maybe_line_directive(&expression.get_span()),
            kind,
        }
    }

    /// Lower the statement-position fall-through: Task/Defer (async value),
    fn lower_expression_statement(&mut self, expression: &Expression) -> Statement {
        let unwrapped = expression.unwrap_parens();
        let statement = if matches!(
            unwrapped,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(unwrapped, ExpressionContext::value());
            LoweredStatement::Body(LoweredBlock {
                statements: value.into_parts().0,
            })
        } else if let Expression::Propagate {
            expression: inner, ..
        } = unwrapped
        {
            LoweredStatement::Body(LoweredBlock {
                statements: self.lower_propagate_statement(inner),
            })
        } else {
            LoweredStatement::Body(LoweredBlock {
                statements: self.lower_discard_value(unwrapped),
            })
        };
        self.directed_at(expression, statement)
    }

    fn lower_while_let_statement(&mut self, expression: &Expression) -> Statement {
        let Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } = expression
        else {
            unreachable!("lower_while_let_statement requires a WhileLet expression");
        };
        let body = self.with_loop(GoExpression::name("_".to_string()), |this| {
            this.lower_while_let(pattern, scrutinee, body)
        });
        self.directed_at(expression, LoweredStatement::Body(body))
    }

    fn lower_infinite_loop(&mut self, body: &Expression) -> LoopPlan {
        self.with_loop(GoExpression::name("_".to_string()), |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        })
    }

    fn lower_condition(&mut self, condition: &Expression) -> (Vec<Statement>, GoExpression) {
        let plan = self.plan_operand(condition, ExpressionContext::value());
        plan.into_parts()
    }

    fn lower_if_condition(&mut self, condition: &Expression) -> (Vec<Statement>, PairCondition) {
        if let Some(fused) = self.lower_fused_predicate_condition(condition) {
            return fused;
        }
        let (setup, rendered) = self.lower_condition(condition);
        (
            setup,
            PairCondition {
                initializer: None,
                condition: rendered,
            },
        )
    }

    fn lower_while(&mut self, condition: &Expression, body: &Expression) -> LoopPlan {
        self.with_loop(GoExpression::name("_".to_string()), |this| {
            let (target, negated) = strip_negations(condition);
            if let Some(exit) = this.lower_fused_predicate_value(target, !negated) {
                let (setup, failure) = exit.into_parts();
                return this.lower_loop_with_exit_test(setup, failure, body);
            }
            let (setup, rendered) = this.lower_condition(condition);
            if !setup.is_empty() {
                let exit = GoExpression::unary(UnaryOp::Not, rendered);
                return this.lower_loop_with_exit_test(setup, exit, body);
            }
            let header = if matches!(
                condition.unwrap_parens(),
                Expression::Literal {
                    literal: Literal::Boolean(true),
                    ..
                }
            ) {
                LoopHeader::Infinite
            } else {
                LoopHeader::While(rendered)
            };
            this.lower_loop_with_header(header, body)
        })
    }

    fn lower_loop_with_exit_test(
        &mut self,
        setup: Vec<Statement>,
        exit: GoExpression,
        body: &Expression,
    ) -> LoopPlan {
        let mut statements = setup;
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                exit,
                LoweredBlock {
                    statements: vec![LoweredStatement::Break(LoopTransfer::Unlabeled).into()],
                },
                ElseArm::None,
            ))
            .into(),
        );
        let lowered_body = self.with_scope(|this| this.lower_block_as_body(body));
        statements.extend(lowered_body.statements);
        self.build_source_loop(LoopHeader::Infinite, LoweredBlock { statements })
    }

    /// Shared loop lowering once the header is known. The caller must have an
    /// active loop context.
    pub(crate) fn lower_loop_with_header(
        &mut self,
        header: LoopHeader,
        body: &Expression,
    ) -> LoopPlan {
        let lowered_body = self.with_scope(|this| this.lower_block_as_body(body));
        self.build_source_loop(header, lowered_body)
    }

    pub(crate) fn build_source_loop(
        &mut self,
        header: LoopHeader,
        mut body: LoweredBlock,
    ) -> LoopPlan {
        let target = self
            .current_loop_id()
            .expect("source loop plan requires an active loop context");
        let label = legalize_source_loop(&mut body, target);
        LoopPlan {
            kind: LoopKind::Source { label },
            header,
            body,
        }
    }

    /// Lower a branch arm body in statement position (`PlacePlan::Statement`).
    pub(crate) fn lower_block_as_body(&mut self, expression: &Expression) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };
        let statements = items
            .iter()
            .enumerate()
            .map(|(index, item)| self.lower_block_item(item, &items[index + 1..]))
            .collect();
        LoweredBlock { statements }
    }

    /// Lower a branching expression (`if`, `if let`, `match`, `select`) into a
    /// `LoweredBlock` targeting `place`. Centralises the dispatch shared by old
    /// emit paths that need to render a branching tail/assignment.
    pub(crate) fn lower_branching_to_block(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let statement =
                    self.lower_if(condition, consequence, alternative.as_deref(), place);
                LoweredBlock {
                    statements: vec![statement.into()],
                }
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => {
                let arms = if_let_match_arms(pattern, consequence, alternative, *span);
                self.lower_match_to_block(scrutinee, &arms, place)
            }
            Expression::Match { subject, arms, .. } => {
                self.lower_match_to_block(subject, arms, place)
            }
            Expression::Select { arms, .. } => LoweredBlock {
                statements: vec![self.lower_select(arms, place).into()],
            },
            _ => unreachable!("lower_branching_to_block: expected if/if-let/match/select"),
        }
    }

    /// Lower a branch arm body into the given place.
    pub(crate) fn lower_block_to_place(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match place {
            PlacePlan::Statement => self.lower_block_as_body(expression),
            PlacePlan::Return => self.lower_block_to_return(expression),
            PlacePlan::Assign { local, target_ty } => {
                self.lower_block_to_assign(expression, local, *target_ty)
            }
        }
    }

    /// Lower a branch arm body in assign position. Fallible (`Result`/`Option`)
    /// targets route through `lower_option_result_assignment`; everything else
    /// flows through the shared `lower_block_to_var`.
    fn lower_block_to_assign(
        &mut self,
        expression: &Expression,
        local: &GoExpression,
        target_ty: Option<&Type>,
    ) -> LoweredBlock {
        if expression.get_type().is_result() || expression.get_type().is_option() {
            return LoweredBlock {
                statements: self.lower_option_result_assignment(local, target_ty, expression),
            };
        }
        LoweredBlock {
            statements: self.lower_block_to_var(expression, local, target_ty, false),
        }
    }

    /// Lower a block in return position: non-tail items become statements,
    /// the tail returns. A tail `let` (`let x = if ...; x`) is elided into the
    /// surrounding return place; function bodies skip that elision.
    fn lower_block_to_return(&mut self, expression: &Expression) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };

        let Some((last, rest)) =
            try_elide_tail_let(items, ElidableTail::Branching).or_else(|| items.split_last())
        else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        let (statements, _) = self.lower_body_until_diverge(rest, last);
        LoweredBlock { statements }
    }

    /// Lower a single tail expression in return position to its return
    /// statements. Shared by branch-arm return lowering and function-body
    /// lowering; leaf values and lowered-ABI returns become `Return` leaves,
    /// `if`/`if let`/`match`/`select` tails recurse structurally with a `Return` place.
    fn lower_return_tail(&mut self, last: &Expression) -> Vec<Statement> {
        let mut statements = Vec::new();
        let return_span = last.get_span();
        let last = if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        };

        if last.get_type().is_unit() {
            if !matches!(last, Expression::Unit { .. }) {
                statements.push(self.lower_statement(last));
            }
            return statements;
        }

        if last.get_type().is_never() {
            return self.lower_never_return_tail(last, &return_span);
        }

        let line = self.maybe_line_directive(&return_span);
        match last {
            Expression::If { .. } | Expression::Select { .. } => {
                let mut block = self.lower_branching_to_block(last, &PlacePlan::Return);
                let statement = block
                    .statements
                    .pop()
                    .expect("if and select lower to one statement");
                statements.extend(directed_first(line, vec![statement]));
            }
            Expression::IfLet { .. } | Expression::Match { .. } => {
                let block = self.lower_branching_to_block(last, &PlacePlan::Return);
                statements.extend(directed_first(line, block.statements));
            }
            Expression::TryBlock { items, ty, .. }
                if let Some(inlined) = self.lower_try_tail_in_place(items, ty) =>
            {
                statements.extend(directed_first(line, inlined));
            }
            _ => {
                let tail = if let Some(tail) = try_emit_lowered_tail_return(self, last) {
                    tail
                } else if let Some(wrapped) = self.lower_wrapped_return(last) {
                    wrapped
                } else {
                    self.lower_plain_return_tail(last)
                };
                statements.extend(directed_first(line, tail));
            }
        }

        statements
    }

    fn lower_never_return_tail(&mut self, last: &Expression, return_span: &Span) -> Vec<Statement> {
        let line = self.maybe_line_directive(return_span);
        directed_first(line, vec![self.lower_statement(last)])
    }

    fn lower_plain_return_tail(&mut self, last: &Expression) -> Vec<Statement> {
        if requires_temp_var(last) {
            let staged = self.plan_operand(last, ExpressionContext::value());
            let (mut statements, value) = staged.into_parts();
            if !value.is_empty() {
                statements.push(plain_return(value));
            }
            statements
        } else {
            let (mut statements, expression) = self.lower_tail_value(last);
            let return_ctx = self.return_ctx();
            let expression =
                self.apply_type_coercion(&mut statements, return_ctx.ty(), last, expression);
            statements.push(plain_return(expression));
            statements
        }
    }

    fn lower_if(
        &mut self,
        condition: &Expression,
        consequence: &Expression,
        alternative: Option<&Expression>,
        place: &PlacePlan,
    ) -> LoweredStatement {
        let (condition_setup, condition) = self.lower_if_condition(condition);

        let then_body = self.with_scope(|this| this.lower_block_to_place(consequence, place));

        let preceding_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_chain(alternative, preceding_diverges, place);

        with_setup(
            condition_setup,
            LoweredStatement::If(IfPlan {
                initializer: condition.initializer,
                condition: condition.condition,
                then_body,
                else_arm,
            }),
        )
    }

    fn lower_else_chain(
        &mut self,
        alternative: Option<&Expression>,
        preceding_diverges: bool,
        place: &PlacePlan,
    ) -> ElseArm {
        let Some(alternative) = alternative else {
            return ElseArm::None;
        };

        if let Expression::If {
            condition,
            consequence,
            alternative: next_alternative,
            ..
        } = alternative
        {
            let (condition_setup, condition) = self.lower_if_condition(condition);

            // Go has no else-if with setup, so the setup and its `if` nest in `} else { ... }`.
            if !condition_setup.is_empty() {
                let body = self.with_scope(|this| {
                    let then_body =
                        this.with_scope(|this| this.lower_block_to_place(consequence, place));
                    let inner = this.lower_else_chain(
                        next_alternative.as_deref(),
                        then_body.ends_with_diverge(),
                        place,
                    );
                    let mut statements = condition_setup;
                    statements.push(
                        LoweredStatement::If(IfPlan {
                            initializer: condition.initializer,
                            condition: condition.condition,
                            then_body,
                            else_arm: inner,
                        })
                        .into(),
                    );
                    LoweredBlock { statements }
                });
                ElseArm::Else {
                    body,
                    inline: false,
                }
            } else {
                let then_body =
                    self.with_scope(|this| this.lower_block_to_place(consequence, place));
                let inner = self.lower_else_chain(
                    next_alternative.as_deref(),
                    preceding_diverges && then_body.ends_with_diverge(),
                    place,
                );
                ElseArm::ElseIf(Box::new(IfPlan {
                    initializer: condition.initializer,
                    condition: condition.condition,
                    then_body,
                    else_arm: inner,
                }))
            }
        } else if preceding_diverges {
            let body =
                self.with_binding_frame(|this| this.lower_block_to_place(alternative, place));
            ElseArm::from_body(body, true)
        } else {
            let body = self.with_scope(|this| this.lower_block_to_place(alternative, place));
            ElseArm::from_body(body, false)
        }
    }
}
