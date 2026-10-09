use super::NativeMethodCall;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::Statement;
use crate::plan::go_expression::BinaryOp;
use crate::plan::values::{CaptureBoundary, GoExpression};

pub(crate) struct BoundsCheckedIndex {
    pub element: GoExpression,
    pub in_bounds: GoExpression,
    pub out_of_bounds: GoExpression,
}

impl Planner<'_> {
    /// `xs.get(i)` as a bounds test and a direct index, each operand evaluated once.
    pub(crate) fn lower_bounds_checked_index(
        &mut self,
        call: &NativeMethodCall<'_>,
    ) -> (Vec<Statement>, BoundsCheckedIndex) {
        let mut receiver = self.plan_operand(call.receiver, ExpressionContext::value());
        if !receiver.effects().can_duplicate() {
            self.pin_staged(&mut receiver, "recv");
        }
        let mut index = self.plan_operand(&call.arguments[0], ExpressionContext::value());
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
        if call.receiver.get_type().is_ref() {
            receiver = GoExpression::dereference(receiver);
        }
        let length = || {
            GoExpression::call(
                GoExpression::name("len".to_string()),
                vec![receiver.clone()],
            )
        };
        let literal = |text: &str| GoExpression::literal(text.to_string());
        let (in_bounds, out_of_bounds) = if index.as_literal() == Some("0") {
            (
                GoExpression::binary(length(), BinaryOp::Gt, literal("0")),
                GoExpression::binary(length(), BinaryOp::Eq, literal("0")),
            )
        } else if index
            .as_literal()
            .is_some_and(|value| value.parse::<u64>().is_ok())
        {
            (
                GoExpression::binary(length(), BinaryOp::Gt, index.clone()),
                GoExpression::binary(length(), BinaryOp::Le, index.clone()),
            )
        } else {
            (
                GoExpression::binary(
                    GoExpression::binary(index.clone(), BinaryOp::Ge, literal("0")),
                    BinaryOp::And,
                    GoExpression::binary(index.clone(), BinaryOp::Lt, length()),
                ),
                GoExpression::binary(
                    GoExpression::binary(index.clone(), BinaryOp::Lt, literal("0")),
                    BinaryOp::Or,
                    GoExpression::binary(index.clone(), BinaryOp::Ge, length()),
                ),
            )
        };
        let element = GoExpression::index(receiver.clone(), index);
        (
            setup,
            BoundsCheckedIndex {
                element,
                in_bounds,
                out_of_bounds,
            },
        )
    }
}
