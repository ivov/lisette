use super::helpers::span_text;
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

const CASE_FUNCTIONS: &[&str] = &["ToLower", "ToUpper"];

pub fn check_manual_equal_fold(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    let negated = match operator {
        BinaryOperator::Equal => false,
        BinaryOperator::NotEqual => true,
        _ => return,
    };

    let (Some((left_fn, namespace, left_arg)), Some((right_fn, _, right_arg))) =
        (case_conversion(left), case_conversion(right))
    else {
        return;
    };

    if left_fn != right_fn {
        return;
    }

    let (Some(namespace_text), Some(left_text), Some(right_text)) = (
        span_text(ctx.source, namespace),
        span_text(ctx.source, left_arg),
        span_text(ctx.source, right_arg),
    ) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::manual_equal_fold(
        span,
        negated,
        namespace_text,
        left_text,
        right_text,
    ));
}

fn case_conversion(expression: &Expression) -> Option<(&str, &Expression, &Expression)> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };

    let [arg] = args.as_slice() else {
        return None;
    };

    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };

    if !CASE_FUNCTIONS.contains(&member.as_str()) {
        return None;
    }

    if namespace.get_type().as_import_namespace() != Some("go:strings") {
        return None;
    }

    Some((member.as_str(), namespace, arg))
}
