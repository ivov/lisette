use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Literal};

use super::helpers::is_float_operand;

pub fn check_float_equality_without_abs(expression: &Expression, ctx: &NodeCtx) {
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
    let (difference, margin) = match operator {
        LessThan | LessThanOrEqual => (left.unwrap_parens(), right.unwrap_parens()),
        GreaterThan | GreaterThanOrEqual => (right.unwrap_parens(), left.unwrap_parens()),
        _ => return,
    };

    let Expression::Binary {
        operator: BinaryOperator::Subtraction,
        left: minuend,
        right: subtrahend,
        span: difference_span,
        ..
    } = difference
    else {
        return;
    };
    if ctx.facts.type_error_spans.contains(difference_span) {
        return;
    }
    if !is_float_operand(ctx.store, minuend) || !is_float_operand(ctx.store, subtrahend) {
        return;
    }

    if !is_positive_numeric_literal(margin) {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::float_equality_without_abs(span));
}

fn is_positive_numeric_literal(expression: &Expression) -> bool {
    match expression {
        Expression::Literal {
            literal: Literal::Float { value, .. },
            ..
        } => *value > 0.0,
        Expression::Literal {
            literal: Literal::Integer { value, .. },
            ..
        } => *value > 0,
        _ => false,
    }
}
