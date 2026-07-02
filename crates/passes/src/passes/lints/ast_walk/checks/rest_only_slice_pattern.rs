use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Pattern, RestPattern};

pub fn check_rest_only_slice_pattern(pattern: &Pattern, ctx: &NodeCtx) {
    if let Pattern::Or { patterns, .. } = pattern {
        for p in patterns {
            check_rest_only_slice_pattern(p, ctx);
        }
        return;
    }

    if let Pattern::Slice {
        prefix, rest, span, ..
    } = pattern
        && prefix.is_empty()
        && !matches!(rest, RestPattern::Absent)
    {
        let replacement = match rest {
            RestPattern::Bind { name, .. } => name.to_string(),
            _ => "_".to_string(),
        };
        let help = format!("Use `let {replacement}` instead");

        ctx.sink.push(
            diagnostics::lint::rest_only_slice_pattern(span, help).with_fix(Fix::new(
                format!("Replace with `{replacement}`"),
                Edit::replacement(*span, replacement),
            )),
        );
    }
}
