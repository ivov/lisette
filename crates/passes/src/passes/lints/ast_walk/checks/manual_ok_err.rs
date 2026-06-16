use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};
use syntax::types::unqualified_name;

use super::helpers::{enum_variant_binding, is_bare_identifier, wraps_binding};

pub fn check_manual_ok_err(expression: &Expression, ctx: &NodeCtx) {
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

    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return;
    }

    if !subject.get_type().is_result() || !expression.get_type().is_option() {
        return;
    }

    let method = if matches_shape(&arms[0], &arms[1], "Ok", "Err") {
        "ok"
    } else if matches_shape(&arms[0], &arms[1], "Err", "Ok") {
        "err"
    } else {
        return;
    };

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink.push(diagnostics::lint::manual_ok_err(
        &match_keyword_span,
        method,
    ));
}

fn matches_shape(a: &MatchArm, b: &MatchArm, keep: &str, discard: &str) -> bool {
    (keeps(a, keep) && discards(b, discard)) || (keeps(b, keep) && discards(a, discard))
}

fn keeps(arm: &MatchArm, variant: &str) -> bool {
    enum_variant_binding(&arm.pattern, variant)
        .is_some_and(|binding| wraps_binding(&arm.expression, "Some", binding))
}

fn discards(arm: &MatchArm, variant: &str) -> bool {
    matches!(&arm.pattern, Pattern::EnumVariant { identifier, .. }
        if unqualified_name(identifier) == variant)
        && is_bare_identifier(&arm.expression, "None")
}
