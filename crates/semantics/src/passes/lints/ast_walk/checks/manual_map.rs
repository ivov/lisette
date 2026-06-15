use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Span};

use super::helpers::{
    enum_variant_binding, has_escaping_control_flow, is_bare_identifier, is_none_pattern,
    wrapped_single_arg, wraps_binding,
};

pub fn check_manual_map(expression: &Expression, ctx: &NodeCtx) {
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

    // The match result must be the same prelude wrapper as the subject, or the
    // arms re-wrap into a look-alike enum (e.g. a user `MyResult` with `Ok`/`Err`
    // variants) that `.map` cannot produce.
    let subject_ty = subject.get_type();
    let result_ty = expression.get_type();
    let fires = if subject_ty.is_option() && result_ty.is_option() {
        check_option_map(&arms[0], &arms[1])
    } else if subject_ty.is_result() && result_ty.is_result() {
        check_result_map(&arms[0], &arms[1])
    } else {
        false
    };

    if !fires {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink
        .push(diagnostics::lint::manual_map(&match_keyword_span));
}

fn check_option_map(arm_a: &MatchArm, arm_b: &MatchArm) -> bool {
    let try_pair = |some_arm: &MatchArm, none_arm: &MatchArm| {
        let Some(binding) = enum_variant_binding(&some_arm.pattern, "Some") else {
            return false;
        };
        is_none_pattern(&none_arm.pattern)
            && is_bare_identifier(&none_arm.expression, "None")
            && is_mapped(&some_arm.expression, "Some", binding)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn check_result_map(arm_a: &MatchArm, arm_b: &MatchArm) -> bool {
    let try_pair = |ok_arm: &MatchArm, err_arm: &MatchArm| {
        let Some(ok_binding) = enum_variant_binding(&ok_arm.pattern, "Ok") else {
            return false;
        };
        let Some(err_binding) = enum_variant_binding(&err_arm.pattern, "Err") else {
            return false;
        };
        wraps_binding(&err_arm.expression, "Err", err_binding)
            && is_mapped(&ok_arm.expression, "Ok", ok_binding)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn is_mapped(expression: &Expression, variant: &str, binding: &str) -> bool {
    let Some(inner) = wrapped_single_arg(expression, variant) else {
        return false;
    };
    // `Some(v) => Some(v)` is the identity map, which is just the subject itself.
    if is_bare_identifier(inner, binding) {
        return false;
    }
    !has_escaping_control_flow(inner)
}
