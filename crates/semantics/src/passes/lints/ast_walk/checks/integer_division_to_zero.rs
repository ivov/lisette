use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};

pub fn check_integer_division_to_zero(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator: BinaryOperator::Division,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (Some(numerator), Some(denominator)) = (
        signed_integer_literal(left.unwrap_parens()),
        signed_integer_literal(right.unwrap_parens()),
    ) else {
        return;
    };

    if numerator != 0 && numerator.unsigned_abs() < denominator.unsigned_abs() {
        ctx.sink
            .push(diagnostics::lint::integer_division_to_zero(span));
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
