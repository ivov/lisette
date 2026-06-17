use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Span};

use super::helpers::{
    enum_variant_binding, is_bare_identifier, is_none_pattern, span_text, wraps_binding,
};

pub fn check_needless_match(expression: &Expression, ctx: &NodeCtx) {
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

    let subject_ty = subject.get_type();
    let result_ty = expression.get_type();
    let reconstructs = if subject_ty.is_option() && result_ty.is_option() {
        option_passthrough(&arms[0], &arms[1])
    } else if subject_ty.is_result() && result_ty.is_result() {
        result_passthrough(&arms[0], &arms[1])
    } else {
        return;
    };

    if !reconstructs {
        return;
    }

    let Some(subject_text) = span_text(ctx.source, subject) else {
        return;
    };

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink.push(diagnostics::lint::needless_match(
        &match_keyword_span,
        subject_text,
    ));
}

fn option_passthrough(a: &MatchArm, b: &MatchArm) -> bool {
    let pair = |some_arm: &MatchArm, none_arm: &MatchArm| {
        rewraps_self(some_arm, "Some")
            && is_none_pattern(&none_arm.pattern)
            && is_bare_identifier(&none_arm.expression, "None")
    };
    pair(a, b) || pair(b, a)
}

fn result_passthrough(a: &MatchArm, b: &MatchArm) -> bool {
    let pair = |ok_arm: &MatchArm, err_arm: &MatchArm| {
        rewraps_self(ok_arm, "Ok") && rewraps_self(err_arm, "Err")
    };
    pair(a, b) || pair(b, a)
}

fn rewraps_self(arm: &MatchArm, variant: &str) -> bool {
    enum_variant_binding(&arm.pattern, variant)
        .is_some_and(|binding| wraps_binding(&arm.expression, variant, binding))
}
