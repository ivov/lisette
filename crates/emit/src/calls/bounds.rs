use super::NativeMethodCall;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::LoweredStatement;
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
    ) -> (Vec<LoweredStatement>, BoundsCheckedIndex) {
        let mut receiver = self.plan_operand(call.receiver, ExpressionContext::value());
        if receiver.evaluation.effect.has_call() {
            self.pin_staged(&mut receiver, "recv");
        }
        let mut index = self.plan_operand(&call.arguments[0], ExpressionContext::value());
        if index.evaluation.effect.has_call() {
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
        let (in_bounds, out_of_bounds) = if index.as_str() == "0" {
            (
                GoExpression::binary(length(), ">", literal("0")),
                GoExpression::binary(length(), "==", literal("0")),
            )
        } else if index.as_str().parse::<u64>().is_ok() {
            (
                GoExpression::binary(length(), ">", index.clone()),
                GoExpression::binary(length(), "<=", index.clone()),
            )
        } else {
            (
                GoExpression::binary(
                    GoExpression::binary(index.clone(), ">=", literal("0")),
                    "&&",
                    GoExpression::binary(index.clone(), "<", length()),
                ),
                GoExpression::binary(
                    GoExpression::binary(index.clone(), "<", literal("0")),
                    "||",
                    GoExpression::binary(index.clone(), ">=", length()),
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
