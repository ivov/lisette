use super::NativeMethodCall;
use super::comma_ok::parenthesize_prefixed;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::CaptureBoundary;

pub(crate) struct BoundsCheckedIndex {
    pub element: String,
    pub in_bounds: String,
    pub out_of_bounds: String,
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
        let (setup, mut values) = sequenced.into_rendered();
        let index = values.pop().expect("index operand");
        let mut receiver = values.pop().expect("receiver operand");
        if call.receiver.get_type().is_ref() {
            receiver = format!("*{receiver}");
        }
        let length = format!("len({receiver})");
        let element = format!("{}[{index}]", parenthesize_prefixed(receiver));
        let (in_bounds, out_of_bounds) = if index == "0" {
            (format!("{length} > 0"), format!("{length} == 0"))
        } else if index.parse::<u64>().is_ok() {
            (
                format!("{length} > {index}"),
                format!("{length} <= {index}"),
            )
        } else {
            (
                format!("{index} >= 0 && {index} < {length}"),
                format!("{index} < 0 || {index} >= {length}"),
            )
        };
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
