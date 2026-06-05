use syntax::ast::Expression;
use syntax::types::peel_to_range_type;

use crate::EmitEffects;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::is_order_sensitive;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use crate::types::native::NativeGoType;

impl Planner<'_> {
    /// Plan an index/slice access. Range-literal and range-typed-variable
    /// slice forms bridge through their string emitters.
    pub(crate) fn plan_index_access(
        &mut self,
        expression: &Expression,
        index: &Expression,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = index
        {
            let (setup, value) =
                self.plan_range_slice(expression, start.as_deref(), end.as_deref(), *inclusive, fx);
            return value_plan_from_statements(setup, value);
        }

        let base_staged = self.stage_base_with_deref(expression, fx);

        // Range-typed variable as index (e.g. `items[r]` where `r: Range<int>`,
        // or `r: Prefix` where `type Prefix = RangeTo<int>`).
        let index_ty = index.get_type();
        if let Some(range_kind) = peel_to_range_type(&index_ty).and_then(|t| t.get_name()) {
            let needs_cap = self.is_native_shape(&expression.get_type(), NativeGoType::Slice);
            let index_staged = self.stage_or_capture(index, "range", fx);
            let (setup, values) =
                self.sequence_structured(vec![base_staged, index_staged], "_base");
            let value = emit_range_var_slice(&values[0], &values[1], range_kind, needs_cap);
            return value_plan_from_statements(setup, value);
        }

        let index_staged = self.stage_composite(index, ExpressionContext::value(), fx);
        let (setup, values) = self.sequence_structured(vec![base_staged, index_staged], "_base");
        value_plan_from_statements(setup, format!("{}[{}]", values[0], values[1]))
    }

    fn stage_base_with_deref(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> StagedExpression {
        let Some(inner) = expression.deref_inner() else {
            return self.stage_operand(expression, ExpressionContext::value(), fx);
        };
        let s = self.stage_operand(inner, ExpressionContext::value(), fx);
        StagedExpression {
            value: format!("(*{})", s.value),
            setup: s.setup,
            capture: s.capture,
            non_literal: s.non_literal,
        }
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
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let needs_cap = self.is_native_shape(&expression.get_type(), NativeGoType::Slice);
        let base_staged = self.stage_base_with_deref(expression, fx);

        let mut all_stages = vec![base_staged];
        if let Some(s) = start {
            all_stages.push(self.stage_operand(s, ExpressionContext::value(), fx));
        }
        if let Some(e) = end {
            all_stages.push(self.stage_operand(e, ExpressionContext::value(), fx));
        }
        let (mut setup, values) = self.sequence_structured(all_stages, "_base");
        let base_str = values[0].clone();

        let (start_str, end_expression) = if start.is_some() {
            (values[1].clone(), values.get(2).map(String::as_str))
        } else {
            (String::new(), values.get(1).map(String::as_str))
        };

        let end_str = match (end_expression, inclusive) {
            (None, _) => String::new(),
            (Some(e), false) => e.to_string(),
            (Some(e), true) => format!("{}+1", e),
        };

        if !needs_cap {
            return (setup, format!("{}[{}:{}]", base_str, start_str, end_str));
        }

        if end_str.is_empty() || end_str.contains('(') {
            let base_expr = expression.deref_inner().unwrap_or(expression);
            let base_str = if is_order_sensitive(base_expr) {
                self.hoist_tmp_value_statement(&mut setup, "base", &base_str)
            } else {
                base_str
            };
            if end_str.is_empty() {
                let len_var = self.hoist_tmp_value_statement(
                    &mut setup,
                    "len",
                    &format!("len({})", base_str),
                );
                return (
                    setup,
                    format!("{}[{}:{}:{}]", base_str, start_str, len_var, len_var),
                );
            }
            let start_str = match start {
                Some(s) if is_order_sensitive(s) => {
                    self.hoist_tmp_value_statement(&mut setup, "start", &start_str)
                }
                _ => start_str,
            };
            let end_var = self.hoist_tmp_value_statement(&mut setup, "end", &end_str);
            return (
                setup,
                format!("{}[{}:{}:{}]", base_str, start_str, end_var, end_var),
            );
        }

        (
            setup,
            format!("{}[{}:{}:{}]", base_str, start_str, end_str, end_str),
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
