use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::program::resolved_definition;

use super::helpers::{expressions_equivalent, is_float_operand, is_side_effect_free};

pub fn check_float_cmp(expression: &Expression, ctx: &NodeCtx) {
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
    let is_equal = match operator {
        Equal => true,
        NotEqual => false,
        _ => return,
    };

    if ctx.facts.type_error_spans.contains(span) {
        return;
    }

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    if !is_float_operand(ctx.store, left) || !is_float_operand(ctx.store, right) {
        return;
    }

    // `x != x` is the NaN idiom, unless re-evaluating the operand can differ.
    if expressions_equivalent(left, right) && is_side_effect_free(left) {
        return;
    }

    // `math.NaN()` is owned by `nan_comparison`.
    if is_exact_operand(left, ctx) || is_exact_operand(right, ctx) {
        return;
    }

    ctx.sink.push(diagnostics::lint::float_cmp(span, is_equal));
}

fn is_exact_operand(expression: &Expression, ctx: &NodeCtx) -> bool {
    if is_float_zero(expression) {
        return true;
    }
    let Expression::Call {
        expression: callee, ..
    } = expression
    else {
        return false;
    };
    matches!(
        resolved_definition(callee, &ctx.facts.resolved_definitions),
        Some("go:math.NaN" | "go:math.Inf")
    )
}

fn is_float_zero(expression: &Expression) -> bool {
    let inner = match expression {
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => expression.unwrap_parens(),
        other => other,
    };
    match inner {
        Expression::Literal {
            literal: Literal::Integer { value: 0, .. },
            ..
        } => true,
        Expression::Literal {
            literal: Literal::Float { value, .. },
            ..
        } => *value == 0.0,
        _ => false,
    }
}
