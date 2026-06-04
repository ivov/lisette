use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};
use syntax::types::unqualified_name;

use super::helpers::{enum_variant_binding, has_escaping_control_flow};

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
            && is_bare_none(&none_arm.expression)
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
        is_err_passthrough(&err_arm.expression, err_binding)
            && is_mapped(&ok_arm.expression, "Ok", ok_binding)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn is_mapped(expression: &Expression, variant: &str, binding: &str) -> bool {
    let Some(inner) = wrapped_argument(expression, variant) else {
        return false;
    };
    // `Some(v) => Some(v)` is the identity map, which is just the subject itself.
    if let Expression::Identifier { value, .. } = inner.unwrap_parens()
        && value.as_str() == binding
    {
        return false;
    }
    !has_escaping_control_flow(inner)
}

fn is_err_passthrough(expression: &Expression, binding: &str) -> bool {
    let Some(inner) = wrapped_argument(expression, "Err") else {
        return false;
    };
    matches!(inner.unwrap_parens(), Expression::Identifier { value, .. }
        if value.as_str() == binding)
}

fn wrapped_argument<'a>(expression: &'a Expression, variant: &str) -> Option<&'a Expression> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let Expression::Identifier { value, .. } = callee.unwrap_parens() else {
        return None;
    };
    if unqualified_name(value) != variant {
        return None;
    }
    Some(&args[0])
}

fn is_none_pattern(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::EnumVariant { identifier, fields, rest, .. }
        if unqualified_name(identifier) == "None" && fields.is_empty() && !*rest)
}

fn is_bare_none(expression: &Expression) -> bool {
    matches!(expression.unwrap_parens(), Expression::Identifier { value, .. }
        if value.as_str() == "None")
}
