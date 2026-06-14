use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use crate::call_target::resolve_call;

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
        ) && (is_math_nan_call(left.unwrap_parens()) || is_math_nan_call(right.unwrap_parens()))
        {
            let always_true = matches!(operator, NotEqual);
            ctx.sink
                .push(diagnostics::infer::nan_comparison(span, always_true));
        }
    }
}

pub(super) fn is_math_nan_call(expression: &Expression) -> bool {
    resolve_call(expression).is_some_and(|target| target.is("go:math", "NaN"))
}
