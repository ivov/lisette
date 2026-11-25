use crate::Planner;
use crate::lookup::facts::component_uses::component_demand;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::control_flow::branches::if_let_match_arms;
use crate::lowering::control_flow::propagation::{ForwardedTail, plain_return};
use crate::lowering::definitions::top_level::ConstPlacement;
use crate::lowering::statements::asserts::is_test_log_call;
use crate::lowering::statements::placement::ElidableTail;
use crate::lowering::statements::placement::try_elide_tail_let;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::output::finish::verify::BodyResult;
use crate::target::ir::bodies::{LoweredBlock, LoweredStatement, Statement, declare};
use crate::target::ir::local::GoIdentifier;
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
        let declaration = declare(result_var.clone(), self.go_type(ty));
        self.scope.declare_go_name(&result_var);
        (result_var, declaration)
    }

    pub(in crate::lowering) fn lower_body_until_diverge(
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
            let statement = self.lower_block_item(item, region.clone());
            let diverged = statement.kind.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                for dropped in region {
                    self.conversions.record_dropped_code(dropped.get_span());
                }
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
            statements.push(directed_at(
                &self.facts,
                check,
                LoweredStatement::Body(LoweredBlock { statements: body }),
            ));
            return (statements, true);
        }
        statements.extend(self.lower_return_tail(last));
        (statements, false)
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
        let return_ctx = self.scope.current_return_ctx();
        if (is_statement_only || is_unit_tail)
            && !body.go_terminates()
            && let Some(return_ty) = return_ctx
                .ty()
                .filter(|ty| !self.facts.resolves_to_unit(ty))
        {
            let return_ty = return_ty.clone();
            let unit = self.unit_value(Some(last), Some(&return_ty));
            body.statements.push(plain_return(unit));
        }

        body
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

        let demand = match &binding.pattern {
            Pattern::Identifier {
                binding: Some(id), ..
            } if mode.else_block().is_none() && !binding.is_mutable() => {
                component_demand(region, *id)
            }
            _ => None,
        };

        let plan = self.build_let_plan(binding, value, mode, demand);
        directed_at(&self.facts, item, LoweredStatement::Body(plan))
    }

    pub(in crate::lowering) fn lower_statement(&mut self, expression: &Expression) -> Statement {
        self.with_rejection_span(expression.get_span(), |this| {
            this.lower_statement_within_span(expression)
        })
    }

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
                    .scope
                    .current_loop()
                    .filter(|loop_context| self.widens_unit(loop_context.result_ty.as_ref()))
                    .and_then(|loop_context| {
                        Some((loop_context.result.clone()?, loop_context.result_ty.clone()))
                    });

                match wide_result {
                    Some((result, result_ty)) => {
                        let mut statements =
                            self.lower_unit_into(expression, None, &result, result_ty.as_ref());
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
            Expression::Call { .. } if is_test_log_call(&self.facts, expression) => {
                self.lower_test_log_statement(expression)
            }
            _ => return None,
        };

        Some(statement)
    }

    fn lower_expression_statement(&mut self, expression: &Expression) -> Statement {
        let statement = if matches!(
            expression,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(expression, ExpressionContext::value());
            LoweredStatement::Body(LoweredBlock {
                statements: value.into_parts().0,
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
        let mut statements = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let statement = self.lower_block_item(item, &items[index + 1..]);
            let diverged = statement.kind.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                for dropped in &items[index + 1..] {
                    self.conversions.record_dropped_code(dropped.get_span());
                }
                break;
            }
        }
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
            statements: self.lower_block_to_var(expression, local, target_ty),
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
