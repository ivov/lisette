use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::is_zero_literal;

pub fn check_manual_bytes_equal(expression: &Expression, ctx: &NodeCtx) {
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

    let (namespace, left_arg, right_arg) = match (
        bytes_compare(left.unwrap_parens()),
        bytes_compare(right.unwrap_parens()),
    ) {
        (Some(call), None) if is_zero_literal(right.unwrap_parens()) => call,
        (None, Some(call)) if is_zero_literal(left.unwrap_parens()) => call,
        _ => return,
    };

    let (Some(namespace_text), Some(left_text), Some(right_text)) = (
        span_text(ctx.source, namespace),
        span_text(ctx.source, left_arg),
        span_text(ctx.source, right_arg),
    ) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::manual_bytes_equal(
        span,
        negated,
        namespace_text,
        left_text,
        right_text,
    ));
}

fn bytes_compare(expression: &Expression) -> Option<(&Expression, &Expression, &Expression)> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return None;
    };

    let [left_arg, right_arg] = args.as_slice() else {
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

    if member.as_str() != "Compare" {
        return None;
    }

    if namespace.get_type().as_import_namespace() != Some("go:bytes") {
        return None;
    }

    Some((namespace, left_arg, right_arg))
}

fn span_text<'a>(source: &'a str, expression: &Expression) -> Option<&'a str> {
    let span = expression.get_span();
    source.get(span.byte_offset as usize..span.end() as usize)
}
