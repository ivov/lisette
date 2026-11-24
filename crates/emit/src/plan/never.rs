use crate::ir::bodies::{LoweredBlock, LoweredStatement, Statement, diverging_statement};
use crate::ir::values::{GoExpression, ValuePlan};
use crate::state::expression::ExpressionContext;
use crate::{Planner, written_span};
use syntax::ast::Expression;
use syntax::types::Type;

pub(crate) fn runs_as_never_statement(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Call { .. }
            | Expression::Block { .. }
            | Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Select { .. }
            | Expression::Loop { .. }
            | Expression::Return { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
    )
}

impl Planner<'_> {
    pub(crate) fn plan_never_value(&mut self, expression: &Expression) -> ValuePlan {
        ValuePlan::computed(
            vec![self.lower_never_value(expression)],
            GoExpression::empty(),
        )
    }

    pub(crate) fn lower_never_value(&mut self, expression: &Expression) -> Statement {
        let Expression::Call { ty, .. } = expression else {
            if runs_as_never_statement(expression) {
                return self.lower_statement(expression);
            }
            let (statements, _) = self
                .plan_operand(expression, ExpressionContext::value())
                .into_parts();
            return LoweredStatement::Body(LoweredBlock { statements }).into();
        };
        let plan = self.plan_call_value(expression, ty, ExpressionContext::value());
        let (mut statements, call) = self
            .facts
            .with_source_positions(&written_span(expression), plan)
            .into_parts();
        if !call.is_empty() {
            statements.push(diverging_statement(call));
        }
        self.facts.directed_at(
            expression,
            LoweredStatement::Body(LoweredBlock { statements }),
        )
    }

    pub(crate) fn fallible_block_type(&self, expression: &Expression, ty: &Type) -> Type {
        if !self.facts.is_never(&ty.ok_type()) {
            return ty.clone();
        }
        self.facts
            .value_use_slot(expression)
            .and_then(|slot| self.facts.peeled_option_or_result(slot))
            .filter(|slot| slot.is_option() == ty.is_option())
            .unwrap_or_else(|| ty.clone())
    }
}
