use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};

use super::helpers::{expression_is_pure, expressions_equivalent};

pub fn check_negated_logical_operand(expression: &Expression, ctx: &NodeCtx) {
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

    let always_true = match operator {
        BinaryOperator::And => false,
        BinaryOperator::Or => true,
        _ => return,
    };

    let left_inner = left.unwrap_parens();
    let right_inner = right.unwrap_parens();

    // The constant rewrite drops the operand, so it must be safe to not evaluate.
    if !expression_is_pure(left_inner, ctx.store) || !expression_is_pure(right_inner, ctx.store) {
        return;
    }

    let Some(operand) = complemented_operand(left_inner, right_inner) else {
        return;
    };

    if !operand.get_type().is_boolean() {
        return;
    }

    ctx.sink.push(diagnostics::lint::negated_logical_operand(
        span,
        always_true,
    ));
}

fn complemented_operand<'a>(a: &'a Expression, b: &'a Expression) -> Option<&'a Expression> {
    if negation_inner(a).is_some_and(|inner| expressions_equivalent(inner, b)) {
        return Some(b);
    }
    if negation_inner(b).is_some_and(|inner| expressions_equivalent(inner, a)) {
        return Some(a);
    }
    None
}

fn negation_inner(expression: &Expression) -> Option<&Expression> {
    if let Expression::Unary {
        operator: UnaryOperator::Not,
        expression: inner,
        ..
    } = expression
    {
        Some(inner.unwrap_parens())
    } else {
        None
    }
}
