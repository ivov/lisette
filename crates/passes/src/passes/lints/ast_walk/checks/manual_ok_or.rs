use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Span};

use super::helpers::{
    enum_variant_binding, has_escaping_control_flow, is_eager_safe, is_none_pattern,
    wrapped_single_arg, wraps_binding,
};

pub fn check_manual_ok_or(expression: &Expression, ctx: &NodeCtx) {
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

    if !subject.get_type().is_option() || !expression.get_type().is_result() {
        return;
    }

    let Some(err_value) = err_arg(&arms[0], &arms[1]).or_else(|| err_arg(&arms[1], &arms[0]))
    else {
        return;
    };

    // `ok_or` evaluates its argument even on the `Some` path, so keep the lazy
    // `ok_or_else` form when that argument does real work.
    let lazy = !is_eager_safe(err_value);
    if lazy && has_escaping_control_flow(err_value) {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink
        .push(diagnostics::lint::manual_ok_or(&match_keyword_span, lazy));
}

fn err_arg<'a>(some_arm: &'a MatchArm, none_arm: &'a MatchArm) -> Option<&'a Expression> {
    let binding = enum_variant_binding(&some_arm.pattern, "Some")?;
    if !wraps_binding(&some_arm.expression, "Ok", binding) {
        return None;
    }
    if !is_none_pattern(&none_arm.pattern) {
        return None;
    }
    wrapped_single_arg(&none_arm.expression, "Err")
}
