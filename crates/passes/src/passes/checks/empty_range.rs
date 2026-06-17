use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, UnaryOperator};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::Range {
        start: Some(start),
        end: Some(end),
        span,
        ..
    } = expression
        && let Some(start_value) = signed_integer_literal(start.unwrap_parens())
        && let Some(end_value) = signed_integer_literal(end.unwrap_parens())
        && start_value > end_value
    {
        ctx.sink.push(diagnostics::infer::empty_range(span));
    }
}

fn signed_integer_literal(expression: &Expression) -> Option<i128> {
    if let Some(value) = expression.as_integer() {
        return Some(value as i128);
    }
    if let Expression::Unary {
        operator: UnaryOperator::Negative,
        expression,
        ..
    } = expression
    {
        return expression.as_integer().map(|value| -(value as i128));
    }
    None
}
