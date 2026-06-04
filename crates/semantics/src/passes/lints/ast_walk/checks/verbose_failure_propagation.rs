use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};
use syntax::types::unqualified_name;

use super::helpers::enum_variant_binding;

pub fn check_verbose_failure_propagation(expression: &Expression, ctx: &NodeCtx) {
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

    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return;
    }

    let subject_ty = subject.get_type();
    let fires = if subject_ty.is_option() {
        check_option_propagation(&arms[0], &arms[1])
    } else if subject_ty.is_result() {
        check_result_propagation(&arms[0], &arms[1])
    } else {
        false
    };

    if fires {
        let keyword_len = match origin {
            MatchOrigin::Explicit => 5,
            MatchOrigin::IfLet { .. } => 2,
        };
        let keyword_span = Span::new(span.file_id, span.byte_offset, keyword_len);
        ctx.sink
            .push(diagnostics::lint::verbose_failure_propagation(
                &keyword_span,
            ));
    }
}

fn check_option_propagation(arm_a: &MatchArm, arm_b: &MatchArm) -> bool {
    let try_pair = |some_arm: &MatchArm, fail_arm: &MatchArm| {
        let Some(name) = enum_variant_binding(&some_arm.pattern, "Some") else {
            return false;
        };
        is_none_or_wildcard(&fail_arm.pattern)
            && body_is_identifier(&some_arm.expression, name)
            && body_is_return_none(&fail_arm.expression)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn check_result_propagation(arm_a: &MatchArm, arm_b: &MatchArm) -> bool {
    let try_pair = |ok_arm: &MatchArm, err_arm: &MatchArm| {
        let Some(ok_name) = enum_variant_binding(&ok_arm.pattern, "Ok") else {
            return false;
        };
        let Some(err_name) = enum_variant_binding(&err_arm.pattern, "Err") else {
            return false;
        };
        body_is_identifier(&ok_arm.expression, ok_name)
            && body_is_return_err(&err_arm.expression, err_name)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn is_none_or_wildcard(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::WildCard { .. } => true,
        Pattern::EnumVariant {
            identifier,
            fields,
            rest,
            ..
        } => unqualified_name(identifier) == "None" && fields.is_empty() && !*rest,
        _ => false,
    }
}

fn body_is_identifier(expression: &Expression, name: &str) -> bool {
    match expression.unwrap_parens() {
        Expression::Identifier { value, .. } => value.as_str() == name,
        Expression::Block { items, .. } => items.len() == 1 && body_is_identifier(&items[0], name),
        _ => false,
    }
}

fn body_is_return_none(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Return {
            expression: inner, ..
        } => matches!(inner.unwrap_parens(), Expression::Identifier { value, .. }
            if value.as_str() == "None"),
        Expression::Block { items, .. } => items.len() == 1 && body_is_return_none(&items[0]),
        _ => false,
    }
}

fn body_is_return_err(expression: &Expression, binding: &str) -> bool {
    match expression.unwrap_parens() {
        Expression::Return {
            expression: inner, ..
        } => is_err_of_binding(inner, binding),
        Expression::Block { items, .. } => {
            items.len() == 1 && body_is_return_err(&items[0], binding)
        }
        _ => false,
    }
}

fn is_err_of_binding(expression: &Expression, binding: &str) -> bool {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return false;
    };
    if args.len() != 1 {
        return false;
    }
    let Expression::Identifier { value, .. } = callee.unwrap_parens() else {
        return false;
    };
    if unqualified_name(value) != "Err" {
        return false;
    }
    matches!(args[0].unwrap_parens(), Expression::Identifier { value, .. }
        if value.as_str() == binding)
}
