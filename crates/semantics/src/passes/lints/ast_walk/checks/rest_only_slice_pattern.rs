use crate::passes::walk::NodeCtx;
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
        let help = match rest {
            RestPattern::Bind { name, .. } => {
                format!("Use `let {}` instead", name)
            }
            _ => "Use `let _` instead".to_string(),
        };

        ctx.sink
            .push(diagnostics::lint::rest_only_slice_pattern(span, help));
    }
}
