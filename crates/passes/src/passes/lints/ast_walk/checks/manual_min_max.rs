use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::SimpleKind;

use super::helpers::{expressions_equivalent, is_side_effect_free};

pub fn check_manual_min_max(expression: &Expression, ctx: &NodeCtx) {
    let Expression::If {
        condition,
        consequence,
        alternative,
        span,
        ..
    } = expression
    else {
        return;
    };
    let Some(alternative) = alternative.as_deref() else {
        return;
    };

    let condition = condition.unwrap_parens();
    let Expression::Binary {
        operator,
        left,
        right,
        span: condition_span,
        ..
    } = condition
    else {
        return;
    };
    let ascending = match operator {
        BinaryOperator::LessThan | BinaryOperator::LessThanOrEqual => true,
        BinaryOperator::GreaterThan | BinaryOperator::GreaterThanOrEqual => false,
        _ => return,
    };
    if ctx.facts.type_error_spans.contains(condition_span) {
        return;
    }

    let (Some(taken), Some(skipped)) = (
        block_single_expression(consequence),
        block_single_expression(alternative),
    ) else {
        return;
    };

    // `a < a` matches both operand mappings, so the min/max choice would be arbitrary.
    if expressions_equivalent(left, right) {
        return;
    }

    let takes_left =
        if expressions_equivalent(taken, left) && expressions_equivalent(skipped, right) {
            true
        } else if expressions_equivalent(taken, right) && expressions_equivalent(skipped, left) {
            false
        } else {
            return;
        };

    // `min(a, b)` evaluates each operand once, the `if` evaluates the winner twice.
    if !is_side_effect_free(left) || !is_side_effect_free(right) {
        return;
    }

    // A float `min` propagates NaN and prefers negative zero, the `if` does neither.
    if !is_integer_or_string(ctx, left) || !is_integer_or_string(ctx, right) {
        return;
    }

    let (name, extreme) = if ascending == takes_left {
        ("min", "smaller")
    } else {
        ("max", "larger")
    };
    ctx.sink
        .push(diagnostics::lint::manual_min_max(span, name, extreme));
}

fn is_integer_or_string(ctx: &NodeCtx, operand: &Expression) -> bool {
    ctx.store
        .underlying_simple_kind(&operand.get_type())
        .is_some_and(|kind| kind == SimpleKind::String || kind.integer_range().is_some())
}

fn block_single_expression(expression: &Expression) -> Option<&Expression> {
    let Expression::Block { items, .. } = expression else {
        return None;
    };
    let [only] = items.as_slice() else {
        return None;
    };
    Some(only)
}
