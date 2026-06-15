use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchOrigin, Pattern, Span};

use super::helpers::{
    enum_variant_binding, has_escaping_control_flow, is_bare_identifier, wraps_binding,
};

pub fn check_manual_filter(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match {
        subject,
        arms,
        origin,
        span,
        ..
    } = expression
    else {
        return;
    };

    if matches!(origin, MatchOrigin::IfLet { .. }) {
        return;
    }

    if arms.len() != 2 {
        return;
    }

    let (keep, dismiss) = (&arms[0], &arms[1]);

    // The guard leaves `Some` not fully covered, so the other arm is a catch-all.
    let Some(guard) = keep.guard.as_deref() else {
        return;
    };
    if dismiss.has_guard() {
        return;
    }

    let Some(binding) = enum_variant_binding(&keep.pattern, "Some") else {
        return;
    };
    if !wraps_binding(&keep.expression, "Some", binding) {
        return;
    }

    let catchall = matches!(
        dismiss.pattern,
        Pattern::WildCard { .. } | Pattern::Identifier { .. }
    );
    if !catchall || !is_bare_identifier(&dismiss.expression, "None") {
        return;
    }

    if !subject.get_type().is_option() || !expression.get_type().is_option() {
        return;
    }

    if has_escaping_control_flow(guard) {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink
        .push(diagnostics::lint::manual_filter(&match_keyword_span));
}
