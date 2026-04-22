use syntax::ast::{Expression, UnaryOperator};

use crate::Emitter;
use crate::utils::Staged;
use crate::write_line;

impl Emitter<'_> {
    pub(crate) fn emit_index_access(
        &mut self,
        output: &mut String,
        expression: &Expression,
        index: &Expression,
    ) -> String {
        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = index
        {
            return self.emit_range_slice(
                output,
                expression,
                start.as_deref(),
                end.as_deref(),
                *inclusive,
            );
        }

        let base_staged = self.stage_base_with_deref(expression);

        // Range-typed variable as index (e.g. `items[r]` where `r: Range<int>`).
        let index_ty = index.get_type();
        if let Some(range_kind) = index_ty.get_name()
            && matches!(
                range_kind,
                "Range" | "RangeInclusive" | "RangeFrom" | "RangeTo" | "RangeToInclusive"
            )
        {
            let needs_cap = expression.get_type().has_name("Slice");
            output.push_str(&base_staged.setup);
            let index_string = self.emit_or_capture(output, index, "range");
            return self.emit_range_var_slice(
                &base_staged.value,
                &index_string,
                range_kind,
                needs_cap,
            );
        }

        let index_staged = self.stage_composite(index);
        let values = self.sequence(output, vec![base_staged, index_staged], "_base");
        format!("{}[{}]", values[0], values[1])
    }

    /// Stage an indexable base expression, unwrapping an explicit deref into
    /// a parenthesized `(*x)` form while preserving evaluation-order setup.
    fn stage_base_with_deref(&mut self, expression: &Expression) -> Staged {
        let Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } = expression
        else {
            return self.stage_operand(expression);
        };
        let s = self.stage_operand(inner);
        Staged {
            value: format!("(*{})", s.value),
            setup: s.setup,
            has_side_effects: s.has_side_effects,
        }
    }

    /// Emit `base[start:end]` (or the three-index form for slices to prevent
    /// append-through-alias corruption). Strings use two-index slicing because
    /// immutability makes the backing array safe to share.
    fn emit_range_slice(
        &mut self,
        output: &mut String,
        expression: &Expression,
        start: Option<&Expression>,
        end: Option<&Expression>,
        inclusive: bool,
    ) -> String {
        let needs_cap = expression.get_type().has_name("Slice");
        let base_staged = self.stage_base_with_deref(expression);

        let mut all_stages = vec![base_staged];
        if let Some(s) = start {
            all_stages.push(self.stage_operand(s));
        }
        if let Some(e) = end {
            all_stages.push(self.stage_operand(e));
        }
        let values = self.sequence(output, all_stages, "_base");
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
            return format!("{}[{}:{}]", base_str, start_str, end_str);
        }

        if end_str.is_empty() {
            let len_var = self.fresh_var(Some("len"));
            self.declare(&len_var);
            write_line!(output, "{} := len({})", len_var, base_str);
            return format!("{}[{}:{}:{}]", base_str, start_str, len_var, len_var);
        }

        if end_str.contains('(') {
            let end_var = self.fresh_var(Some("end"));
            self.declare(&end_var);
            write_line!(output, "{} := {}", end_var, end_str);
            return format!("{}[{}:{}:{}]", base_str, start_str, end_var, end_var);
        }

        format!("{}[{}:{}:{}]", base_str, start_str, end_str, end_str)
    }

    /// Emit a Go slice expression from a range-typed variable index.
    ///
    /// When `needs_cap` is true, appends a third index to cap capacity at
    /// length, preventing append-through-alias corruption on shared backing
    /// arrays. Range field accesses (e.g. `.End`) are pure, so repeating
    /// them in the cap position is safe.
    fn emit_range_var_slice(
        &self,
        base: &str,
        range: &str,
        range_kind: &str,
        needs_cap: bool,
    ) -> String {
        let (start, end) = match range_kind {
            "Range" => (format!("{}.Start", range), format!("{}.End", range)),
            "RangeInclusive" => (format!("{}.Start", range), format!("{}.End+1", range)),
            "RangeFrom" => (format!("{}.Start", range), String::new()),
            "RangeTo" => (String::new(), format!("{}.End", range)),
            "RangeToInclusive" => (String::new(), format!("{}.End+1", range)),
            _ => unreachable!("unexpected range kind: {}", range_kind),
        };

        if !needs_cap {
            return format!("{}[{}:{}]", base, start, end);
        }

        // For open-ended ranges, cap at len(base).
        let cap = if end.is_empty() {
            format!("len({})", base)
        } else {
            end.clone()
        };

        format!("{}[{}:{}:{}]", base, start, end, cap)
    }
}
