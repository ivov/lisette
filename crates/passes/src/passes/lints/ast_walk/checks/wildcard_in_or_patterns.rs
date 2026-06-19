use crate::passes::walk::NodeCtx;
use syntax::ast::{Pattern, collect_pattern_bindings};

pub fn check_wildcard_in_or_patterns(pattern: &Pattern, ctx: &NodeCtx) {
    let Pattern::Or { patterns, span } = pattern else {
        return;
    };

    if patterns.len() < 2
        || !patterns
            .iter()
            .any(|p| matches!(p, Pattern::WildCard { .. }))
    {
        return;
    }

    // A binding alternative means the checker already rejected the or-pattern.
    if patterns
        .iter()
        .any(|p| !collect_pattern_bindings(p).is_empty())
    {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::wildcard_in_or_patterns(span));
}
