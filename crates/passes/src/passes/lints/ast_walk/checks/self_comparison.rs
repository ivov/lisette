use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::is_float_operand;

pub fn check_self_comparison(expression: &Expression, ctx: &NodeCtx) {
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

    let (
        Expression::Identifier {
            value: left_name, ..
        },
        Expression::Identifier {
            value: right_name, ..
        },
    ) = (left.unwrap_parens(), right.unwrap_parens())
    else {
        return;
    };

    if left_name != right_name {
        return;
    }

    // NaN == NaN is false per IEEE 754, so skip floats.
    if is_float_operand(ctx.store, left) {
        return;
    }

    let always_true = matches!(operator, Equal | LessThanOrEqual | GreaterThanOrEqual);
    ctx.sink.push(diagnostics::lint::tautological_comparison(
        span,
        always_true,
    ));
}
