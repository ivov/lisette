use syntax::ast::Expression;
use syntax::types::peel_to_range_type;

use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::is_order_sensitive;
use crate::plan::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::types::native::NativeGoType;

impl Planner<'_> {
    /// Plan an index/slice access. Range-literal and range-typed-variable
    /// slice forms bridge through their string emitters.
    pub(crate) fn plan_index_access(
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
            return self.plan_range_slice(expression, start.as_deref(), end.as_deref(), *inclusive);
        }

        let mut base_staged = self.stage_base_with_deref(expression);

        // Range-typed variable as index (e.g. `items[r]` where `r: Range<int>`,
        // or `r: Prefix` where `type Prefix = RangeTo<int>`).
        let index_ty = index.get_type();
        if let Some(range_kind) = peel_to_range_type(&index_ty).and_then(|t| t.get_name()) {
            let needs_cap = self.is_native_shape(&expression.get_type(), NativeGoType::Slice);
            if base_staged.evaluation.effect.has_call() {
                self.pin_staged(&mut base_staged, "_base");
            }
            let index_staged = self.stage_or_capture(index, "range");
            let sequenced = self.sequence_values(
                vec![base_staged, index_staged],
                CaptureBoundary::SiblingSequence,
                "_base",
            );
            let effect = sequenced.effect;
            let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
            let (setup, values) = sequenced.into_rendered();
            let value = emit_range_var_slice(&values[0], &values[1], range_kind, needs_cap);
            return ValuePlan::computed(
                setup,
                GoExpression::opaque_with_deferred_evaluation(value, contains_deferred_evaluation),
                effect,
            );
        }

        self.sequence_indexed_access(expression, base_staged, index, "_base")
    }

    pub(crate) fn sequence_indexed_access(
        &mut self,
        base: &Expression,
        mut base_staged: ValuePlan,
        index: &Expression,
        prefix: &str,
    ) -> ValuePlan {
        let index_staged = self.stage_composite(index, ExpressionContext::value());
        if base_staged.setup.is_empty()
            && is_order_sensitive(base)
            && (base_staged.evaluation.effect.has_call()
                || index_staged.evaluation.effect.has_call())
        {
            self.pin_staged(&mut base_staged, prefix);
        }
        let sequenced = self.sequence_values(
            vec![base_staged, index_staged],
            CaptureBoundary::SiblingSequence,
            prefix,
        );
        let effect = sequenced.effect;
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let base = values.next().expect("indexed access has a base");
        let index = values.next().expect("indexed access has an index");
        ValuePlan::computed(setup, GoExpression::index(base, index), effect)
    }

    pub(crate) fn stage_base_with_deref(&mut self, expression: &Expression) -> ValuePlan {
        let Some(inner) = expression.deref_inner() else {
            return self.stage_operand(expression, ExpressionContext::value());
        };
        let mut staged = self
            .stage_operand(inner, ExpressionContext::value())
            .unary("*")
            .parenthesized();
        staged.make_observable();
        staged
    }

    /// Plan `base[start:end]` (or the three-index form for slices to prevent
    /// append-through-alias corruption). Strings use two-index slicing because
    /// immutability makes the backing array safe to share.
    fn plan_range_slice(
        &mut self,
        expression: &Expression,
        start: Option<&Expression>,
        end: Option<&Expression>,
        inclusive: bool,
    ) -> ValuePlan {
        let needs_cap = self.is_native_shape(&expression.get_type(), NativeGoType::Slice);
        let base_staged = self.stage_base_with_deref(expression);

        let mut all_stages = vec![base_staged];
        if let Some(s) = start {
            all_stages.push(self.stage_operand(s, ExpressionContext::value()));
        }
        if let Some(e) = end {
            all_stages.push(self.stage_operand(e, ExpressionContext::value()));
        }
        let sequenced = self.sequence_values(all_stages, CaptureBoundary::SiblingSequence, "_base");
        let effect = sequenced.effect;
        let mut setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let mut base = values.next().expect("slice access has a base");
        let mut start_value = start.map(|_| values.next().expect("slice access has a start"));
        let mut end_value = end.map(|_| values.next().expect("slice access has an end"));
        if inclusive && let Some(end_expression) = end_value.take() {
            end_value = Some(GoExpression::compact_binary(
                end_expression,
                "+",
                GoExpression::literal("1".to_string()),
            ));
        }

        if !needs_cap {
            return ValuePlan::computed(
                setup,
                GoExpression::slice(base, start_value.as_ref(), end_value.as_ref(), None),
                effect,
            );
        }

        if end_value
            .as_ref()
            .is_none_or(GoExpression::contains_deferred_evaluation)
        {
            let base_expr = expression.deref_inner().unwrap_or(expression);
            if is_order_sensitive(base_expr) {
                base = GoExpression::name(self.hoist_tmp_value_statement(
                    &mut setup,
                    "base",
                    &base.rendered(),
                ));
            }
            let Some(end_expression) = end_value else {
                let length = GoExpression::name(self.hoist_tmp_value_statement(
                    &mut setup,
                    "len",
                    &format!("len({})", base.rendered()),
                ));
                return ValuePlan::computed(
                    setup,
                    GoExpression::slice(base, start_value.as_ref(), Some(&length), Some(&length)),
                    effect,
                );
            };
            if start.is_some_and(is_order_sensitive) {
                start_value = start_value.map(|start_expression| {
                    GoExpression::name(self.hoist_tmp_value_statement(
                        &mut setup,
                        "start",
                        &start_expression.rendered(),
                    ))
                });
            }
            let end_variable = GoExpression::name(self.hoist_tmp_value_statement(
                &mut setup,
                "end",
                &end_expression.rendered(),
            ));
            return ValuePlan::computed(
                setup,
                GoExpression::slice(
                    base,
                    start_value.as_ref(),
                    Some(&end_variable),
                    Some(&end_variable),
                ),
                effect,
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
            effect,
        )
    }
}

pub(crate) fn range_var_bounds(
    range_var: &str,
    range_kind: &str,
) -> (Option<String>, Option<String>) {
    match range_kind {
        "Range" => (
            Some(format!("{}.Start", range_var)),
            Some(format!("{}.End", range_var)),
        ),
        "RangeInclusive" => (
            Some(format!("{}.Start", range_var)),
            Some(format!("{}.End+1", range_var)),
        ),
        "RangeFrom" => (Some(format!("{}.Start", range_var)), None),
        "RangeTo" => (None, Some(format!("{}.End", range_var))),
        "RangeToInclusive" => (None, Some(format!("{}.End+1", range_var))),
        _ => unreachable!("unexpected range kind: {}", range_kind),
    }
}

/// Slice expression from a range-typed variable. `needs_cap` adds a
/// third index that caps capacity at length to block append-through-alias
/// corruption; range field accesses (`.End`) are pure, so repeating them
/// in the cap position is safe.
fn emit_range_var_slice(base: &str, range: &str, range_kind: &str, needs_cap: bool) -> String {
    let (start, end) = range_var_bounds(range, range_kind);
    let start_str = start.as_deref().unwrap_or("");
    let end_str = end.as_deref().unwrap_or("");

    if !needs_cap {
        return format!("{}[{}:{}]", base, start_str, end_str);
    }

    let bound = if end_str.is_empty() {
        format!("len({})", base)
    } else {
        end_str.to_string()
    };

    format!("{}[{}:{}:{}]", base, start_str, bound, bound)
}
