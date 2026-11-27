use crate::Planner;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::expressions::operand_order::SequencedArray;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::Statement;
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use syntax::ast::Expression;

#[derive(Clone, Copy)]
pub(in crate::lowering) struct IndexSite<'a> {
    pub(in crate::lowering) receiver: &'a Expression,
    pub(in crate::lowering) index: &'a Expression,
}

pub(in crate::lowering) struct BoundsCheckedIndex {
    receiver: GoExpression,
    index: GoExpression,
}

impl BoundsCheckedIndex {
    pub(super) fn element(&self) -> GoExpression {
        GoExpression::index(self.receiver.clone(), self.index.clone())
    }

    pub(super) fn condition(&self, polarity: Polarity) -> GoExpression {
        let length = GoExpression::call(
            GoExpression::external_name("len".to_string()),
            vec![self.receiver.clone()],
        );
        if let Some(literal) = self.index.as_integer().filter(|value| *value >= 0) {
            let operator = match polarity {
                Polarity::Positive => GoBinaryOperator::GreaterThan,
                Polarity::Negated if literal == 0 => GoBinaryOperator::Equal,
                Polarity::Negated => GoBinaryOperator::LessThanOrEqual,
            };
            return GoExpression::binary(length, operator, self.index.clone());
        }
        let (lower, join, upper) = match polarity {
            Polarity::Positive => (
                GoBinaryOperator::GreaterThanOrEqual,
                GoBinaryOperator::And,
                GoBinaryOperator::LessThan,
            ),
            Polarity::Negated => (
                GoBinaryOperator::LessThan,
                GoBinaryOperator::Or,
                GoBinaryOperator::GreaterThanOrEqual,
            ),
        };
        GoExpression::binary(
            GoExpression::binary(self.index.clone(), lower, GoExpression::integer(0)),
            join,
            GoExpression::binary(self.index.clone(), upper, length),
        )
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_bounds_checked_index(
        &mut self,
        site: IndexSite<'_>,
    ) -> (Vec<Statement>, BoundsCheckedIndex) {
        let source = site.receiver;
        let mut receiver = self.plan_operand(source, ExpressionContext::value());
        if receiver.effects().must_evaluate_once() {
            self.pin_to_temp(&mut receiver, "recv");
        }
        let mut index = self.plan_operand(site.index, ExpressionContext::value());
        if index.effects().must_evaluate_once() {
            self.pin_to_temp(&mut index, "idx");
        }
        let SequencedArray {
            setup,
            values: [mut receiver, index],
            ..
        } = self.sequence_value_array([receiver, index], CaptureBoundary::SiblingSequence, "arg");
        if self.facts.is_reference(&source.get_type()) {
            receiver = GoExpression::dereference(receiver);
        }
        (setup, BoundsCheckedIndex { receiver, index })
    }
}
