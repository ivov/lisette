use syntax::ast::Expression;
use syntax::types::peel_to_range_type;

use crate::EmitEffects;
use crate::Planner;
use crate::ReturnContext;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
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
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = index
        {
            let (setup, value) = self.plan_range_slice(
                expression,
                start.as_deref(),
                end.as_deref(),
                *inclusive,
                ambient,
                fx,
            );
            return value_plan_from_statements(setup, value);
        }

        let base_staged = self.stage_base_with_deref(expression, ambient, fx);

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

        let index_staged = self.stage_composite(
            index,
            ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
            fx,
        );
        let (setup, values) = self.sequence_structured(vec![base_staged, index_staged], "_base");
        value_plan_from_statements(setup, format!("{}[{}]", values[0], values[1]))
    }

    fn stage_base_with_deref(
        &mut self,
        expression: &Expression,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> StagedExpression {
        let Some(inner) = expression.deref_inner() else {
            return self.stage_operand(
                expression,
                ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                fx,
            );
        };
        let s = self.stage_operand(
            inner,
            ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
            fx,
        );
        StagedExpression {
            value: format!("(*{})", s.value),
            setup: s.setup,
            capture: s.capture,
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
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let needs_cap = self.is_native_shape(&expression.get_type(), NativeGoType::Slice);
        let base_staged = self.stage_base_with_deref(expression, ambient, fx);

        let mut all_stages = vec![base_staged];
        if let Some(s) = start {
            all_stages.push(self.stage_operand(
                s,
                ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                fx,
            ));
        }
        if let Some(e) = end {
            all_stages.push(self.stage_operand(
                e,
                ExpressionContext::value().with_ambient_return_ctx_opt(ambient),
                fx,
            ));
        }
        let (mut setup, values) = self.sequence_structured(all_stages, "_base");
        let base_str = &values[0];

        let (start_str, end_expression) = if start.is_some() {
            (values[1].as_str(), values.get(2).map(|s| s.as_str()))
        } else {
            ("", values.get(1).map(|s| s.as_str()))
        };

        let end_str = match (end_expression, inclusive) {
            (None, _) => String::new(),
            (Some(e), false) => e.to_string(),
            (Some(e), true) => format!("{}+1", e),
        };

        if !needs_cap {
            return (setup, format!("{}[{}:{}]", base_str, start_str, end_str));
        }

        if end_str.is_empty() {
            let len_var =
                self.hoist_tmp_value_statement(&mut setup, "len", &format!("len({})", base_str));
            return (
                setup,
                format!("{}[{}:{}:{}]", base_str, start_str, len_var, len_var),
            );
        }

        if end_str.contains('(') {
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

    let cap = if end_str.is_empty() {
        format!("len({})", base)
    } else {
        end_str.to_string()
    };

    format!("{}[{}:{}:{}]", base, start_str, end_str, cap)
}
