use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::{flip_comparison, is_zero_literal};

pub fn check_unsigned_comparison(expression: &Expression, ctx: &NodeCtx) {
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

    use BinaryOperator::*;
    if !matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    ) {
        return;
    }

    let operator = match (
        is_zero_literal(left.unwrap_parens()),
        is_zero_literal(right.unwrap_parens()),
    ) {
        (true, false)
            if ctx
                .store
                .underlying_simple_kind(&right.get_type())
                .is_some_and(|kind| kind.is_unsigned_int()) =>
        {
            flip_comparison(*operator)
        }
        (false, true)
            if ctx
                .store
                .underlying_simple_kind(&left.get_type())
                .is_some_and(|kind| kind.is_unsigned_int()) =>
        {
            *operator
        }
        _ => return,
    };

    let always_true = match operator {
        LessThan => false,
        GreaterThanOrEqual => true,
        _ => return,
    };

    ctx.sink
        .push(diagnostics::lint::unsigned_comparison(span, always_true));
}
