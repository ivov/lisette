use crate::Planner;
use crate::lookup::facts::component_uses::component_demand;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::control_flow::branches::if_let_match_arms;
use crate::lowering::control_flow::propagation::{ForwardedTail, plain_return};
use crate::lowering::definitions::top_level::ConstPlacement;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::asserts::test_log_call;
use crate::lowering::statements::placement::ElidableTail;
use crate::lowering::statements::placement::try_elide_tail_let;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::output::finish::verify::BodyResult;
use crate::target::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, declare, directed_first,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::Fallible;
use crate::target::ir::values::GoExpression;
use std::{iter, slice};
use syntax::ast::{Expression, Pattern};
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn operand_temp_declaration(
        &mut self,
        ty: &Type,
    ) -> (GoIdentifier, Statement) {
        let result_var = self.fresh_local("tmp");
        let declaration = declare(result_var.clone(), self.queries().go_type(ty));
        self.scope.declare_go_name(&result_var);
        (result_var, declaration)
    }

    pub(in crate::lowering) fn lower_block_items(
        &mut self,
        rest: &[Expression],
        last: &Expression,
        tail: BlockTail<'_>,
    ) -> Vec<Statement> {
        let mut statements: Vec<Statement> = Vec::with_capacity(rest.len() + 1);
        let forwarded = match tail {
            BlockTail::Value(TailTarget::Return) => rest.split_last().and_then(|(check, rest)| {
                Some((check, rest, self.queries().forwarded_tail(check, last)?))
            }),
            BlockTail::Statement | BlockTail::Value(_) => None,
        };
        let lowered = forwarded.map_or(rest, |(_, rest, _)| rest);
        for (index, item) in lowered.iter().enumerate() {
            let region = rest[index + 1..].iter().chain(iter::once(last));
            let statement = self.lower_block_item(item, region.clone());
            let diverged = statement.kind.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                for dropped in region {
                    self.conversions.record_dropped_code(dropped.get_span());
                }
                return statements;
            }
        }
        if let Some((check, _, forwarded)) = forwarded {
            statements.push(self.lower_forwarded_tail(check, forwarded));
            return statements;
        }
        match tail {
            BlockTail::Statement => statements.push(self.lower_block_item(last, iter::empty())),
            BlockTail::Value(target) => statements.extend(self.lower_block_tail(last, &target)),
        }
        statements
    }

    fn lower_forwarded_tail(&mut self, check: &Expression, tail: ForwardedTail<'_>) -> Statement {
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
        directed_at(
            &self.facts,
            check,
            LoweredStatement::Body(LoweredBlock { statements: body }),
        )
    }

    fn lower_block_tail(&mut self, last: &Expression, target: &TailTarget<'_>) -> Vec<Statement> {
        let line = self.facts.source_position(&last.get_span());
        let statements = match (target, self.queries().tail_kind(last)) {
            (TailTarget::Return, _) => self.lower_return(last),
            (_, TailKind::Diverges) => self.lower_never_value(last),
            (TailTarget::Assign { target, target_ty }, TailKind::Unit) => {
                self.lower_assign_unit_tail(last, target, target_ty)
            }
            (TailTarget::Assign { target, target_ty }, TailKind::Value) => {
                self.lower_assign_value_tail(last, target, target_ty)
            }
            (TailTarget::Try(fallible), TailKind::Unit) => {
                let mut statements = self.lower_unit_effect(last);
                statements.extend(self.lower_try_unit_return(fallible));
                statements
            }
            (TailTarget::Try(fallible), TailKind::Value) => {
                self.lower_try_value_tail(last, fallible)
            }
        };
        directed_first(line, statements)
    }

    pub(in crate::lowering) fn lower_unit_effect(&mut self, tail: &Expression) -> Vec<Statement> {
        if let Expression::Unit { .. } = tail {
            return Vec::new();
        }
        vec![self.lower_statement(tail)]
    }

    pub(in crate::lowering) fn lower_function_body(
        &mut self,
        body: &Expression,
        result: BodyResult,
    ) -> LoweredBlock {
        if result == BodyResult::Void {
            return self.lower_block_as_body(body);
        }

        if let Some(ty) = self.scope.current_return_ctx().ty()
            && self.facts.resolves_to_unit(ty)
        {
            let mut block = self.lower_block_as_body(body);
            if !block.go_terminates() {
                block
                    .statements
                    .push(plain_return(self.unit_value(None, Some(ty))));
            }
            return block;
        }

        let items: &[Expression] = if let Expression::Block { items, .. } = body {
            items
        } else {
            slice::from_ref(body)
        };

        let Some((last, rest)) =
            try_elide_tail_let(&self.facts, items, ElidableTail::FallibleBlock)
                .or_else(|| items.split_last())
        else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        LoweredBlock {
            statements: self.lower_block_items(rest, last, BlockTail::Value(TailTarget::Return)),
        }
    }

    pub(in crate::lowering) fn lower_block_item<'r>(
        &mut self,
        item: &Expression,
        region: impl IntoIterator<Item = &'r Expression>,
    ) -> Statement {
        self.with_rejection_span(item.get_span(), |this| {
            this.lower_block_item_within_span(item, region)
        })
    }

    fn lower_block_item_within_span<'r>(
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

        let demand = if let Pattern::Identifier {
            binding: Some(id), ..
        } = &binding.pattern
            && mode.else_block().is_none()
            && !binding.is_mutable()
        {
            component_demand(region, *id)
        } else {
            None
        };

        let plan = self.build_let_plan(binding, value, mode, demand);
        directed_at(&self.facts, item, LoweredStatement::Body(plan))
    }

    pub(in crate::lowering) fn lower_statement(&mut self, expression: &Expression) -> Statement {
        self.with_rejection_span(expression.get_span(), |this| {
            this.lower_statement_within_span(expression)
        })
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    fn lower_statement_within_span(&mut self, expression: &Expression) -> Statement {
        match expression {
            Expression::Block { items, .. } if items.is_empty() => {
                LoweredStatement::Body(LoweredBlock {
                    statements: Vec::new(),
                })
                .into()
            }
            Expression::Block { .. } => LoweredStatement::Block(
                self.with_scope(|this| this.lower_block_as_body(expression)),
            )
            .into(),
            Expression::For {
                binding,
                iterable,
                body,
                ..
            } => self.lower_for_statement(expression, binding, iterable, body),
            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                ..
            } => self.lower_while_let_statement(expression, pattern, scrutinee, body),
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
                let statement = LoweredStatement::Const(self.lower_const(
                    identifier,
                    *binding,
                    value,
                    ty,
                    ConstPlacement::Local,
                ));
                directed_at(&self.facts, expression, statement)
            }
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::ImplBlock { .. } => {
                unreachable!("the parser rejects item definitions inside function bodies")
            }
            _ => match self.try_lower_directed_statement(expression) {
                Some(statement) => directed_at(&self.facts, expression, statement),
                None => self.lower_expression_statement(expression),
            },
        }
    }

    #[allow(clippy::wildcard_enum_match_arm)]
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
            Expression::Continue { .. } => {
                LoweredStatement::Continue(self.queries().current_loop_transfer())
            }
            Expression::Break { value: None, .. } => self.lower_bare_break(expression),
            Expression::Break {
                value: Some(value), ..
            } => LoweredStatement::Body(self.build_break_value_plan(value)),
            Expression::Return {
                expression: value, ..
            } => LoweredStatement::Body(LoweredBlock {
                statements: self.lower_return(value),
            }),
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
            Expression::Assert {
                expression: operand,
                ..
            } => self.lower_assert_statement(expression, operand),
            Expression::Call { .. } => {
                let call = test_log_call(&self.facts, expression)?;
                self.lower_test_log_statement(call)
            }
            _ => return None,
        };

        Some(statement)
    }

    fn lower_bare_break(&mut self, expression: &Expression) -> LoweredStatement {
        let transfer = LoweredStatement::Break(self.queries().current_loop_transfer());
        let wide_result = self
            .scope
            .current_loop()
            .and_then(|loop_context| loop_context.result.clone())
            .filter(|slot| !self.facts.resolves_to_unit(&slot.ty));

        match wide_result {
            Some(slot) => {
                let mut statements = self.lower_unit_into(expression, None, &slot.target, &slot.ty);
                statements.push(transfer.into());
                LoweredStatement::Body(LoweredBlock { statements })
            }
            None => transfer,
        }
    }

    fn lower_expression_statement(&mut self, expression: &Expression) -> Statement {
        let statement = if matches!(
            expression,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(expression, ExpressionContext::value());
            LoweredStatement::Body(LoweredBlock {
                statements: value.into_statements(),
            })
        } else if let Expression::Propagate {
            expression: inner, ..
        } = expression
        {
            LoweredStatement::Body(LoweredBlock {
                statements: self.lower_propagate_statement(inner),
            })
        } else {
            LoweredStatement::Body(LoweredBlock {
                statements: self.lower_discard_value(expression),
            })
        };

        directed_at(&self.facts, expression, statement)
    }

    pub(in crate::lowering) fn lower_block_as_body(
        &mut self,
        expression: &Expression,
    ) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };
        let statements = match items.split_last() {
            Some((last, rest)) => self.lower_block_items(rest, last, BlockTail::Statement),
            None => Vec::new(),
        };
        LoweredBlock { statements }
    }

    pub(in crate::lowering) fn lower_block_to_place(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match place {
            PlacePlan::Statement => self.lower_block_as_body(expression),
            PlacePlan::Return => self.lower_block_to_return(expression),
            PlacePlan::Assign { local, target_ty } => {
                self.lower_block_to_assign(expression, local, target_ty)
            }
        }
    }

    fn lower_block_to_assign(
        &mut self,
        expression: &Expression,
        local: &GoExpression,
        target_ty: &Type,
    ) -> LoweredBlock {
        if self.facts.is_option_or_result(&expression.get_type()) {
            return LoweredBlock {
                statements: self.lower_option_result_assignment(local, target_ty, expression),
            };
        }
        LoweredBlock {
            statements: self.lower_block_to_var(expression, local, target_ty),
        }
    }
}

pub(in crate::lowering) enum BlockTail<'a> {
    Statement,
    Value(TailTarget<'a>),
}

pub(in crate::lowering) enum TailTarget<'a> {
    Return,
    Assign {
        target: &'a GoExpression,
        target_ty: &'a Type,
    },
    Try(&'a Fallible),
}

pub(in crate::lowering) enum TailKind {
    Value,
    Unit,
    Diverges,
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn tail_kind(&self, tail: &Expression) -> TailKind {
        let ty = tail.get_type();
        if self.facts.is_never(&ty) {
            TailKind::Diverges
        } else if self.facts.produces_no_go_value(tail) {
            TailKind::Unit
        } else {
            TailKind::Value
        }
    }
}

pub(in crate::lowering) fn directed_at(
    facts: &EmitFacts<'_>,
    expression: &Expression,
    kind: LoweredStatement,
) -> Statement {
    Statement {
        line: facts.source_position(&expression.get_span()),
        kind,
    }
}
