use super::NativeCallContext;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::Statement;
use crate::plan::go_expression::BinaryOp;
use crate::plan::values::{CaptureBoundary, GoExpression};

pub(crate) struct BoundsCheckedIndex {
    receiver: GoExpression,
    index: GoExpression,
}

impl BoundsCheckedIndex {
    pub(crate) fn element(&self) -> GoExpression {
        GoExpression::index(self.receiver.clone(), self.index.clone())
    }

    pub(crate) fn condition(&self, in_bounds: bool) -> GoExpression {
        let length = GoExpression::call(
            GoExpression::external_name("len".to_string()),
            vec![self.receiver.clone()],
        );
        if let Some(literal) = self
            .index
            .as_literal()
            .filter(|value| value.parse::<u64>().is_ok())
        {
            let operator = if in_bounds {
                BinaryOp::Gt
            } else if literal == "0" {
                BinaryOp::Eq
            } else {
                BinaryOp::Le
            };
            return GoExpression::binary(length, operator, self.index.clone());
        }
        let (lower, join, upper) = if in_bounds {
            (BinaryOp::Ge, BinaryOp::And, BinaryOp::Lt)
        } else {
            (BinaryOp::Lt, BinaryOp::Or, BinaryOp::Ge)
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
    /// `xs.get(i)` as a bounds test and a direct index, each operand evaluated once.
    pub(crate) fn lower_bounds_checked_index(
        &mut self,
        call: &NativeCallContext<'_>,
    ) -> (Vec<Statement>, BoundsCheckedIndex) {
        let (source, arguments) = call
            .receiver_and_arguments()
            .expect("index call has a receiver");
        let mut receiver = self.plan_operand(source, ExpressionContext::value());
        if !receiver.effects().can_duplicate() {
            self.pin_staged(&mut receiver, "recv");
        }
        let mut index = self.plan_operand(&arguments[0], ExpressionContext::value());
        if !index.effects().can_duplicate() {
            self.pin_staged(&mut index, "idx");
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
        if source.get_type().is_ref() {
            receiver = GoExpression::dereference(receiver);
        }
        (setup, BoundsCheckedIndex { receiver, index })
    }
}
