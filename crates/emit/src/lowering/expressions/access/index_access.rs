use crate::target::names::go_name;
use syntax::ast::Expression;
use syntax::program::NativeTypeKind;
use syntax::types::RangeKind;

use crate::Planner;
use crate::lookup::facts::queries::is_order_sensitive;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::go_expression::BinaryOp;
use crate::target::ir::values::{CaptureBoundary, GoExpression};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RangeEnd {
    Exclusive,
    Inclusive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapacityClip {
    Clip,
    Keep,
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_index_access(
        &mut self,
        expression: &Expression,
        index: &Expression,
    ) -> ValuePlan {
        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = index
        {
            let range_end = if *inclusive {
                RangeEnd::Inclusive
            } else {
                RangeEnd::Exclusive
            };
            return self.plan_range_slice(
                expression,
                start.as_deref(),
                end.as_deref(),
                range_end,
                CapacityClip::Clip,
            );
        }

        let mut base_operand = self.plan_base_with_deref(expression);

        if let Some(range_kind) = self.facts.checked(index).range_kind() {
            let needs_cap = self
                .facts
                .is_native_shape(&expression.get_type(), NativeTypeKind::Slice);
            if base_operand.effects().must_evaluate_once() {
                self.pin_to_temp(&mut base_operand, "base");
            }
            let index_operand = self.plan_or_capture(index, "range");
            let sequenced = self.sequence_values(
                vec![base_operand, index_operand],
                CaptureBoundary::SiblingSequence,
                "base",
            );
            let mut values = sequenced.values.into_iter();
            let base = values.next().expect("range index has a base");
            let range = values.next().expect("range index has a range");
            let value = range_var_slice(base, &range, range_kind, needs_cap);
            return ValuePlan::computed(sequenced.setup, value);
        }

        self.sequence_indexed_access(expression, base_operand, index, "base")
    }

    fn sequence_indexed_access(
        &mut self,
        base: &Expression,
        mut base_operand: ValuePlan,
        index: &Expression,
        prefix: &'static str,
    ) -> ValuePlan {
        let index_operand = self.plan_composite_value(index, ExpressionContext::value());
        let index_effects = index_operand.effects();
        let index_is_fixed = index_operand.can_delay() && !index_effects.runs_code();
        if base_operand.setup().is_empty()
            && is_order_sensitive(base)
            && ((base_operand.effects().runs_effectful_code() && !index_is_fixed)
                || index_effects.runs_effectful_code())
        {
            self.pin_to_temp(&mut base_operand, prefix);
        }
        let sequenced = self.sequence_values(
            vec![base_operand, index_operand],
            CaptureBoundary::SiblingSequence,
            prefix,
        );
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let base = values.next().expect("indexed access has a base");
        let index = values.next().expect("indexed access has an index");
        ValuePlan::computed(setup, GoExpression::index(base, index))
    }

    pub(in crate::lowering) fn plan_copied_spread(&mut self, spread: &Expression) -> ValuePlan {
        if let Expression::IndexedAccess {
            expression, index, ..
        } = spread
            && let Expression::Range {
                start,
                end,
                inclusive,
                ..
            } = index.as_ref()
        {
            let range_end = if *inclusive {
                RangeEnd::Inclusive
            } else {
                RangeEnd::Exclusive
            };
            return self.plan_range_slice(
                expression,
                start.as_deref(),
                end.as_deref(),
                range_end,
                CapacityClip::Keep,
            );
        }
        self.plan_operand(spread, ExpressionContext::value())
    }

    fn plan_base_with_deref(&mut self, expression: &Expression) -> ValuePlan {
        let Some(inner) = expression.deref_inner() else {
            return self.plan_operand(expression, ExpressionContext::value());
        };
        let mut operand = self
            .plan_operand(inner, ExpressionContext::value())
            .dereference();
        operand.make_observable();
        operand
    }

    fn plan_range_slice(
        &mut self,
        expression: &Expression,
        start: Option<&Expression>,
        end: Option<&Expression>,
        range_end: RangeEnd,
        capacity: CapacityClip,
    ) -> ValuePlan {
        let needs_cap = capacity == CapacityClip::Clip
            && self
                .facts
                .is_native_shape(&expression.get_type(), NativeTypeKind::Slice);
        let base_operand = self.plan_base_with_deref(expression);

        let mut all_operands = vec![base_operand];
        if let Some(s) = start {
            all_operands.push(self.plan_operand(s, ExpressionContext::value()));
        }
        if let Some(e) = end {
            all_operands.push(self.plan_operand(e, ExpressionContext::value()));
        }
        let runs_effectful_code = all_operands
            .iter()
            .any(|operand| operand.effects().runs_effectful_code());
        let sequenced =
            self.sequence_values(all_operands, CaptureBoundary::SiblingSequence, "base");
        let mut setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let mut base = values.next().expect("slice access has a base");
        let mut start_value = start.map(|_| values.next().expect("slice access has a start"));
        let mut end_value = end.map(|_| values.next().expect("slice access has an end"));
        if range_end == RangeEnd::Inclusive
            && let Some(end_expression) = end_value.take()
        {
            end_value = Some(GoExpression::binary(
                end_expression,
                BinaryOp::Add,
                GoExpression::literal("1".to_string()),
            ));
        }

        if !needs_cap {
            return ValuePlan::computed(
                setup,
                GoExpression::slice(base, start_value.as_ref(), end_value.as_ref(), None),
            );
        }

        if end_value
            .as_ref()
            .is_none_or(|end| end.effects().must_evaluate_once())
        {
            let base_expr = expression.deref_inner().unwrap_or(expression);
            if base.effects().must_evaluate_once()
                || (is_order_sensitive(base_expr) && runs_effectful_code)
            {
                base = GoExpression::identifier(self.hoist_temp(&mut setup, "base", base));
            }
            let Some(end_expression) = end_value else {
                let length = GoExpression::identifier(self.hoist_temp(
                    &mut setup,
                    "len",
                    GoExpression::call(
                        GoExpression::external_name("len".to_string()),
                        vec![base.clone()],
                    ),
                ));

                return ValuePlan::computed(
                    setup,
                    GoExpression::slice(base, start_value.as_ref(), Some(&length), Some(&length)),
                );
            };

            if start.is_some_and(is_order_sensitive) {
                start_value = start_value.map(|start_expression| {
                    GoExpression::identifier(self.hoist_temp(&mut setup, "start", start_expression))
                });
            }
            let end_variable =
                GoExpression::identifier(self.hoist_temp(&mut setup, "end", end_expression));
            return ValuePlan::computed(
                setup,
                GoExpression::slice(
                    base,
                    start_value.as_ref(),
                    Some(&end_variable),
                    Some(&end_variable),
                ),
            );
        }

        ValuePlan::computed(
            setup,
            GoExpression::slice(
                base,
                start_value.as_ref(),
                end_value.as_ref(),
                end_value.as_ref(),
            ),
        )
    }
}

pub(in crate::lowering) fn range_var_bounds(
    range: &GoExpression,
    range_kind: RangeKind,
) -> (Option<GoExpression>, Option<GoExpression>) {
    let start = || GoExpression::selector(range.clone(), go_name::RANGE_START_FIELD.to_string());
    let end = || GoExpression::selector(range.clone(), go_name::RANGE_END_FIELD.to_string());
    let end_inclusive =
        || GoExpression::binary(end(), BinaryOp::Add, GoExpression::literal("1".to_string()));
    match range_kind {
        RangeKind::Range => (Some(start()), Some(end())),
        RangeKind::RangeInclusive => (Some(start()), Some(end_inclusive())),
        RangeKind::RangeFrom => (Some(start()), None),
        RangeKind::RangeTo => (None, Some(end())),
        RangeKind::RangeToInclusive => (None, Some(end_inclusive())),
    }
}

fn range_var_slice(
    base: GoExpression,
    range: &GoExpression,
    range_kind: RangeKind,
    needs_cap: bool,
) -> GoExpression {
    let (start, end) = range_var_bounds(range, range_kind);
    if !needs_cap {
        return GoExpression::slice(base, start.as_ref(), end.as_ref(), None);
    }
    let bound = end.unwrap_or_else(|| {
        GoExpression::call(
            GoExpression::external_name("len".to_string()),
            vec![base.clone()],
        )
    });

    GoExpression::slice(base, start.as_ref(), Some(&bound), Some(&bound))
}
