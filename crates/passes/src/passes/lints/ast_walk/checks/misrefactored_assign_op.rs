use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::{expressions_equivalent, is_side_effect_free, span_text};

pub fn check_misrefactored_assign_op(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Assignment {
        target,
        value,
        compound_operator: Some(compound),
        span,
    } = expression
    else {
        return;
    };

    // A compound assignment is lowered to `target <op> rhs`, so the written
    // right-hand side is this binary's right operand.
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = value.unwrap_parens()
    else {
        return;
    };
    if operator != compound {
        return;
    }

    let target = target.unwrap_parens();
    if !expressions_equivalent(target, left) || !is_side_effect_free(target) {
        return;
    }

    let Expression::Binary {
        operator: rhs_operator,
        left: rhs_left,
        right: rhs_right,
        ..
    } = right.unwrap_parens()
    else {
        return;
    };
    if rhs_operator != compound {
        return;
    }

    // The mirror form `a op= b op a` only collapses to `a op= b` for a commutative
    // `op`; a non-commutative one (`a -= b - a`) would get a wrong rewrite.
    let other = if expressions_equivalent(target, rhs_left) {
        rhs_right
    } else if is_commutative(*compound) && expressions_equivalent(target, rhs_right) {
        rhs_left
    } else {
        return;
    };

    let Some(symbol) = compound.compound_assignment_symbol() else {
        return;
    };
    let (Some(target_text), Some(other_text)) =
        (span_text(ctx.source, target), span_text(ctx.source, other))
    else {
        return;
    };

    ctx.sink.push(diagnostics::lint::misrefactored_assign_op(
        span,
        target_text,
        symbol,
        other_text,
    ));
}

fn is_commutative(operator: BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(
        operator,
        Addition | Multiplication | BitwiseAnd | BitwiseOr | BitwiseXor
    )
}
