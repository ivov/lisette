use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::{expressions_equivalent, is_side_effect_free, span_text};

pub fn check_duplicate_logical_operand(expression: &Expression, ctx: &NodeCtx) {
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

    if !matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
        return;
    }

    let left_inner = left.unwrap_parens();
    let right_inner = right.unwrap_parens();

    // `f() && f()` may be intentional double-invocation; only warn when both
    // sides have no observable effect.
    if !is_side_effect_free(left_inner) || !is_side_effect_free(right_inner) {
        return;
    }

    if !expressions_equivalent(left_inner, right_inner) {
        return;
    }

    let Some(operand_text) = span_text(ctx.source(), left) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::duplicate_logical_operand(
        span,
        operand_text,
    ));
}
