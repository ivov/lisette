use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::program::resolved_definition;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    {
        use BinaryOperator::*;
        if matches!(
            operator,
            Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
        ) && (is_math_nan_call(left.unwrap_parens(), ctx)
            || is_math_nan_call(right.unwrap_parens(), ctx))
        {
            let always_true = matches!(operator, NotEqual);
            ctx.sink
                .push(diagnostics::infer::nan_comparison(span, always_true));
        }
    }
}

pub(super) fn is_math_nan_call(expression: &Expression, ctx: &NodeCtx) -> bool {
    let Expression::Call {
        expression: callee, ..
    } = expression
    else {
        return false;
    };
    resolved_definition(callee, &ctx.facts.resolved_definitions) == Some("go:math.NaN")
}
