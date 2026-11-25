use super::NativeCallContext;
use crate::Planner;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::plan::context::ExpressionContext;
use crate::target::ir::bodies::Statement;
use crate::target::ir::go_expression::BinaryOp;
use crate::target::ir::values::{CaptureBoundary, GoExpression};

pub(in crate::lowering) struct BoundsCheckedIndex {
    receiver: GoExpression,
    index: GoExpression,
}

impl BoundsCheckedIndex {
    pub(in crate::lowering) fn element(&self) -> GoExpression {
        GoExpression::index(self.receiver.clone(), self.index.clone())
    }

    pub(in crate::lowering) fn condition(&self, polarity: Polarity) -> GoExpression {
        let length = GoExpression::call(
            GoExpression::external_name("len".to_string()),
            vec![self.receiver.clone()],
        );
        if let Some(literal) = self
            .index
            .as_literal()
            .filter(|value| value.parse::<u64>().is_ok())
        {
            let operator = match polarity {
                Polarity::Positive => BinaryOp::Gt,
                Polarity::Negated if literal == "0" => BinaryOp::Eq,
                Polarity::Negated => BinaryOp::Le,
            };
            return GoExpression::binary(length, operator, self.index.clone());
        }
        let (lower, join, upper) = match polarity {
            Polarity::Positive => (BinaryOp::Ge, BinaryOp::And, BinaryOp::Lt),
            Polarity::Negated => (BinaryOp::Lt, BinaryOp::Or, BinaryOp::Ge),
        };
        GoExpression::binary(
            GoExpression::binary(
                self.index.clone(),
                lower,
                GoExpression::literal("0".to_string()),
            ),
            join,
            GoExpression::binary(self.index.clone(), upper, length),
        )
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_bounds_checked_index(
        &mut self,
        call: &NativeCallContext<'_>,
    ) -> (Vec<Statement>, BoundsCheckedIndex) {
        let (source, arguments) = call
            .receiver_and_arguments()
            .expect("index call has a receiver");
        let mut receiver = self.plan_operand(source, ExpressionContext::value());
        if receiver.effects().must_evaluate_once() {
            self.pin_to_temp(&mut receiver, "recv");
        }
        let mut index = self.plan_operand(&arguments[0], ExpressionContext::value());
        if index.effects().must_evaluate_once() {
            self.pin_to_temp(&mut index, "idx");
        }
        let sequenced = self.sequence_values(
            vec![receiver, index],
            CaptureBoundary::SiblingSequence,
            "arg",
        );
        let setup = sequenced.setup;
        let mut values = sequenced.values;
        let index = values.pop().expect("index operand");
        let mut receiver = values.pop().expect("receiver operand");
        if self.facts.is_reference(&source.get_type()) {
            receiver = GoExpression::dereference(receiver);
        }
        (setup, BoundsCheckedIndex { receiver, index })
    }
}
