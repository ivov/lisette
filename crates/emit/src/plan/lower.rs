use crate::EmitFacts;
use crate::Planner;
use crate::analyze::component_uses::component_demand;
use crate::calls::bound_value::PairCondition;
use crate::calls::predicates::strip_negations;
use crate::control_flow::propagation::{ForwardedTail, plain_return};
use crate::control_flow::targets::legalize_source_loop;
use crate::definitions::toplevel::ConstPlacement;
use crate::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock,
    LoweredStatement, PlacePlan, Statement, declare, directed_first, with_setup,
};
use crate::ir::go_expression::{BinaryOp, UnaryOp};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{GoExpression, ValuePlan};
use crate::plan::placement::{
    ElidableTail, collapse_declared_temp, peel_value_wrappers, requires_temp_var,
};
use crate::state::expression::ExpressionContext;
use std::iter;
use std::slice;
use syntax::ast::{
    BinaryOperator, Expression, ExpressionId, IfLetAlternative, Literal, MatchArm, Pattern, Span,
};
use syntax::types::Type;

fn if_let_match_arms(
    pattern: &Pattern,
    consequence: &Expression,
    alternative: &IfLetAlternative,
    span: Span,
    implicit_unit: Option<ExpressionId>,
) -> Vec<MatchArm> {
    let alternative = alternative
        .expression()
        .cloned()
        .unwrap_or_else(|| Expression::Unit {
            id: implicit_unit.expect("internal error: an `if let` with no `else` has its unit"),
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

pub(crate) struct Guard {
    setup: Vec<Statement>,
    condition: PairCondition,
    then_body: LoweredBlock,
}

impl Guard {
    fn into_if(self) -> IfPlan {
        IfPlan {
            initializer: self.condition.initializer,
            condition: self.condition.condition,
            then_body: self.then_body,
            else_arm: ElseArm::None,
            else_if_line: None,
            origin: IfOrigin::User,
        }
    }

    pub(crate) fn into_statement(self) -> LoweredStatement {
        let Guard {
            setup,
            condition,
            then_body,
        } = self;
        let guard = Guard {
            setup: Vec::new(),
            condition,
            then_body,
        };
        with_setup(setup, LoweredStatement::If(guard.into_if()))
    }
}

fn collect_conjuncts<'e>(condition: &'e Expression, conjuncts: &mut Vec<&'e Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::And,
        left,
        right,
        ..
    } = condition.unwrap_parens()
    {
        collect_conjuncts(left, conjuncts);
        collect_conjuncts(right, conjuncts);
    } else {
        conjuncts.push(condition);
    }
}

pub(crate) fn is_conjunction(condition: &Expression) -> bool {
    matches!(
        condition.unwrap_parens(),
        Expression::Binary {
            operator: BinaryOperator::And,
            ..
        }
    )
}

impl Planner<'_> {
    pub(crate) fn operand_temp_declaration(&mut self, ty: &Type) -> (GoIdentifier, Statement) {
        let result_var = self.fresh_local(None);
        let declaration = declare(result_var.clone(), self.go_type(ty));
        self.declare(&result_var);
        (result_var, declaration)
    }

    pub(crate) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let ty = self.facts.value_use_slot(expression).unwrap_or(ty);
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let target = GoExpression::identifier(result_var.clone());
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
            self.facts.short_declaration_keeps_type(ty),
        );
        ValuePlan::captured(setup, result_var)
    }

    pub(crate) fn plan_loop_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let Expression::Loop { body, .. } = expression else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let result = GoExpression::identifier(result_var.clone());
        let plan = self.with_loop(result, Some(ty.clone()), |this| {
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
            .and_then(|(check, rest)| Some((check, rest, self.forwarded_tail(check, last)?)));
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
        if let Some((check, _, tail)) = forwarded {
            let (mut body, value) = match tail {
                ForwardedTail::ErrorCall(call) => {
                    self.plan_call_expression(call, None, ExpressionContext::value())
                }
                ForwardedTail::TaggedResult(result) => {
                    self.plan_value(result, ExpressionContext::value())
                }
            }
            .into_parts();
            body.push(plain_return(value));
            statements.push(self.facts.directed_at(
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

        if let Some(ty) = self.return_ctx().ty()
            && self.facts.resolves_to_unit(ty)
        {
            let mut block = self.lower_block_as_body(body);
            if !block.go_terminates() {
                block.statements.push(plain_return(self.go_zero(ty)));
            }
            return block;
        }

        let items: &[Expression] = if let Expression::Block { items, .. } = body {
            items
        } else {
            slice::from_ref(body)
        };

        let Some((last, rest)) = self
            .facts
            .try_elide_tail_let(items, ElidableTail::FallibleBlock)
            .or_else(|| items.split_last())
        else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        let (statements, diverged) = self.lower_body_until_diverge(rest, last);
        if diverged {
            return LoweredBlock { statements };
        }

        let is_statement_only = matches!(
            last,
            Expression::Assignment { .. } | Expression::Let { .. } | Expression::Const { .. }
        );
        let is_unit_tail = !is_statement_only
            && !matches!(last, Expression::Return { .. })
            && self.facts.resolves_to_unit(&last.get_type());
        let mut body = LoweredBlock { statements };
        let return_ctx = self.return_ctx();
        if (is_statement_only || is_unit_tail)
            && !body.go_terminates()
            && let Some(return_ty) = return_ctx
                .ty()
                .filter(|ty| !self.facts.resolves_to_unit(ty))
        {
            let return_ty = return_ty.clone();
            let zero = self.go_zero(&return_ty);
            body.statements.push(plain_return(zero));
        }

        body
    }

    pub(crate) fn lower_block_item<'r>(
        &mut self,
        item: &Expression,
        region: impl IntoIterator<Item = &'r Expression>,
    ) -> Statement {
        self.focused(item.get_span(), |this| {
            this.lower_focused_block_item(item, region)
        })
    }

    fn lower_focused_block_item<'r>(
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
        self.facts.directed_at(item, LoweredStatement::Body(plan))
    }

    pub(crate) fn lower_statement(&mut self, expression: &Expression) -> Statement {
        self.focused(expression.get_span(), |this| {
            this.lower_focused_statement(expression)
        })
    }

    fn lower_focused_statement(&mut self, expression: &Expression) -> Statement {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::Block { .. } => LoweredStatement::Block(
                self.with_scope(|this| this.lower_block_as_body(expression)),
            )
            .into(),
            Expression::For { .. } => self.lower_for_statement(expression),
            Expression::WhileLet { .. } => self.lower_while_let_statement(expression),
            Expression::Const {
                binding,
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let Some(value) = value.value() else {
                    return LoweredStatement::Block(LoweredBlock { statements: vec![] }).into();
                };
                let statement =
                    self.lower_const(identifier, *binding, value, ty, ConstPlacement::Local);
                self.facts.directed_at(expression, statement)
            }
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::ImplBlock { .. } => {
                unreachable!("the parser rejects item definitions inside function bodies")
            }
            _ => match self.try_lower_directed_statement(expression) {
                Some(statement) => self.facts.directed_at(expression, statement),
                None => self.lower_expression_statement(expression),
            },
        }
    }

    fn try_lower_directed_statement(
        &mut self,
        expression: &Expression,
    ) -> Option<LoweredStatement> {
        let statement = match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => self.lower_if(
                condition,
                consequence,
                alternative.as_deref(),
                &PlacePlan::Statement,
            ),
            Expression::Loop { body, .. } => LoweredStatement::Loop(self.lower_infinite_loop(body)),
            Expression::While {
                condition, body, ..
            } => LoweredStatement::Loop(self.lower_while(condition, body)),
            Expression::Continue { .. } => LoweredStatement::Continue(self.current_loop_transfer()),
            Expression::Break { value: None, .. } => {
                let transfer = LoweredStatement::Break(self.current_loop_transfer());
                let wide_result = self
                    .current_loop()
                    .filter(|loop_context| self.widens_unit(loop_context.result_ty.as_ref()))
                    .map(|loop_context| loop_context.result.clone());
                match wide_result {
                    Some(result) => {
                        let mut statements = self.lower_unit_into(None, &result);
                        statements.push(transfer.into());
                        LoweredStatement::Body(LoweredBlock { statements })
                    }
                    None => transfer,
                }
            }
            Expression::Break {
                value: Some(value), ..
            } => LoweredStatement::Body(self.build_break_value_plan(value)),
            Expression::Return {
                expression: value, ..
            } => LoweredStatement::Body(self.build_return_plan(value)),
            Expression::Let {
                binding,
                value,
                mode,
                ..
            } => LoweredStatement::Body(self.build_let_plan(binding, value, mode, None)),
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => self.build_assignment_plan(target, value, compound_operator.as_ref()),
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => {
                let implicit_unit = self.facts.checked(expression).implicit_unit();
                let arms =
                    if_let_match_arms(pattern, consequence, alternative, *span, implicit_unit);
                LoweredStatement::Body(self.lower_match_to_block(
                    scrutinee,
                    &arms,
                    &PlacePlan::Statement,
                ))
            }
            Expression::Match { subject, arms, .. } => LoweredStatement::Body(
                self.lower_match_to_block(subject, arms, &PlacePlan::Statement),
            ),
            Expression::Select { arms, span, .. } => {
                self.lower_select(arms, span, &PlacePlan::Statement)
            }
            Expression::Assert { .. } => self.lower_assert_statement(expression),
            Expression::Call { .. } if self.facts.is_test_log_call(expression) => {
                self.lower_test_log_statement(expression)
            }
            _ => return None,
        };
        Some(statement)
    }

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
        self.facts.directed_at(expression, statement)
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
        let body = self.with_loop(GoExpression::external_name("_".to_string()), None, |this| {
            this.lower_while_let(pattern, scrutinee, body)
        });
        self.facts
            .directed_at(expression, LoweredStatement::Body(body))
    }

    fn lower_infinite_loop(&mut self, body: &Expression) -> LoopPlan {
        self.with_loop(GoExpression::external_name("_".to_string()), None, |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        })
    }

    fn plan_condition(&mut self, condition: &Expression) -> (Vec<Statement>, GoExpression) {
        let plan = self.plan_operand(condition, ExpressionContext::value());
        plan.into_parts()
    }

    fn lower_if_condition(&mut self, condition: &Expression) -> (Vec<Statement>, PairCondition) {
        if let Some(fused) = self.try_plan_fused_predicate_condition(condition) {
            return fused;
        }
        let (setup, rendered) = self.plan_condition(condition);
        (
            setup,
            PairCondition {
                initializer: None,
                condition: rendered,
            },
        )
    }

    pub(crate) fn lower_conjunction_guard(
        &mut self,
        condition: &Expression,
        body: impl FnOnce(&mut Self) -> LoweredBlock,
    ) -> Guard {
        let mut conjuncts = Vec::new();
        collect_conjuncts(condition, &mut conjuncts);
        let mut levels: Vec<(Vec<Statement>, PairCondition)> = Vec::new();
        for conjunct in conjuncts {
            let (setup, next) = self.lower_if_condition(conjunct);
            if setup.is_empty()
                && next.initializer.is_none()
                && let Some((outer_setup, current)) = levels.pop()
            {
                let condition =
                    GoExpression::binary(current.condition, BinaryOp::And, next.condition);
                let merged = PairCondition {
                    initializer: current.initializer,
                    condition,
                };
                levels.push((outer_setup, merged));
            } else {
                levels.push((setup, next));
                self.enter_scope();
            }
        }
        let mut then_body = body(self);
        let mut levels = levels.into_iter().rev().peekable();
        loop {
            let (setup, condition) = levels.next().expect("a conjunction has a first operand");
            self.exit_scope();
            let guard = Guard {
                setup,
                condition,
                then_body,
            };
            if levels.peek().is_none() {
                return guard;
            }
            then_body = LoweredBlock {
                statements: vec![guard.into_statement().into()],
            };
        }
    }

    fn lower_while(&mut self, condition: &Expression, body: &Expression) -> LoopPlan {
        self.with_loop(GoExpression::external_name("_".to_string()), None, |this| {
            let (target, negated) = strip_negations(condition);
            if let Some(exit) = this.try_plan_fused_predicate_value(target, !negated) {
                let (setup, failure) = exit.into_parts();
                return this.lower_loop_with_exit_test(setup, failure, body);
            }
            let (setup, rendered) = this.plan_condition(condition);
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
                let implicit_unit = self.facts.checked(expression).implicit_unit();
                let arms =
                    if_let_match_arms(pattern, consequence, alternative, *span, implicit_unit);
                self.lower_match_to_block(scrutinee, &arms, place)
            }
            Expression::Match { subject, arms, .. } => {
                self.lower_match_to_block(subject, arms, place)
            }
            Expression::Select { arms, span, .. } => LoweredBlock {
                statements: vec![self.lower_select(arms, span, place).into()],
            },
            _ => unreachable!("lower_branching_to_block: expected if/if-let/match/select"),
        }
    }

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

    fn lower_block_to_assign(
        &mut self,
        expression: &Expression,
        local: &GoExpression,
        target_ty: Option<&Type>,
    ) -> LoweredBlock {
        if self.facts.is_option_or_result(&expression.get_type()) {
            return LoweredBlock {
                statements: self.lower_option_result_assignment(local, target_ty, expression),
            };
        }
        LoweredBlock {
            statements: self.lower_block_to_var(expression, local, target_ty, false),
        }
    }

    fn lower_block_to_return(&mut self, expression: &Expression) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };

        let Some((last, rest)) = self
            .facts
            .try_elide_tail_let(items, ElidableTail::Branching)
            .or_else(|| items.split_last())
        else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        let (statements, _) = self.lower_body_until_diverge(rest, last);
        LoweredBlock { statements }
    }

    fn lower_return_tail(&mut self, last: &Expression) -> Vec<Statement> {
        let mut statements = Vec::new();
        let return_span = last.get_span();
        let last = peel_value_wrappers(if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        });

        if self.facts.resolves_to_unit(&last.get_type()) {
            if !matches!(last, Expression::Unit { .. }) {
                statements.push(self.lower_statement(last));
            }
            return statements;
        }

        if last.get_type().is_never() {
            return self.lower_never_return_tail(last, &return_span);
        }

        let line = self.facts.source_position(&return_span);
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
            Expression::TryBlock { .. }
                if let Some(inlined) = self.try_lower_try_tail_in_place(last) =>
            {
                statements.extend(directed_first(line, inlined));
            }
            _ => {
                let tail = if let Some(tail) = self.try_lower_layout_return(last) {
                    tail
                } else if let Some(wrapped) = self.try_lower_wrapped_return(last) {
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
        let line = self.facts.source_position(return_span);
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
            let (mut statements, expression) = self.plan_tail_value(last);
            let expression = if self.tail_tuple_return_slots(last).is_some() {
                expression
            } else {
                let return_ctx = self.return_ctx();
                self.apply_type_coercion(&mut statements, return_ctx.ty(), last, expression)
            };
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
        if alternative.is_none() && is_conjunction(condition) {
            return self
                .lower_conjunction_guard(condition, |this| {
                    this.with_scope(|this| this.lower_block_to_place(consequence, place))
                })
                .into_statement();
        }

        let (condition_setup, condition) = self.lower_if_condition(condition);

        let then_place = if alternative.is_none() {
            &PlacePlan::Statement
        } else {
            place
        };
        let then_body = self.with_scope(|this| this.lower_block_to_place(consequence, then_place));

        let preceding_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_chain(alternative, preceding_diverges, place);

        with_setup(
            condition_setup,
            LoweredStatement::If(IfPlan {
                initializer: condition.initializer,
                condition: condition.condition,
                then_body,
                else_arm,
                else_if_line: None,
                origin: IfOrigin::User,
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
            let line = self.facts.source_position(&alternative.get_span());
            if next_alternative.is_none() && is_conjunction(condition) {
                return self.with_scope(|this| {
                    let guard = this.lower_conjunction_guard(condition, |this| {
                        this.with_scope(|this| this.lower_block_to_place(consequence, place))
                    });
                    if guard.setup.is_empty() {
                        let mut plan = guard.into_if();
                        plan.else_if_line = line;
                        ElseArm::ElseIf(Box::new(plan))
                    } else {
                        ElseArm::Else {
                            body: LoweredBlock {
                                statements: directed_first(
                                    line,
                                    vec![guard.into_statement().into()],
                                ),
                            },
                            inline: false,
                        }
                    }
                });
            }
            let (condition_setup, condition) = self.lower_if_condition(condition);

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
                            else_if_line: None,
                            origin: IfOrigin::User,
                        })
                        .into(),
                    );
                    LoweredBlock {
                        statements: directed_first(line, statements),
                    }
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
                    else_if_line: line,
                    origin: IfOrigin::User,
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

impl EmitFacts<'_> {
    fn directed_at(&self, expression: &Expression, kind: LoweredStatement) -> Statement {
        Statement {
            line: self.source_position(&expression.get_span()),
            kind,
        }
    }
}
