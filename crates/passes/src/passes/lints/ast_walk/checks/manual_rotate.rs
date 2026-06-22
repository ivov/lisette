use super::helpers::{expressions_equivalent, is_side_effect_free};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::SimpleKind;

pub fn check_manual_rotate(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator: BinaryOperator::BitwiseOr,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (Some(first), Some(second)) = (as_shift(left), as_shift(right)) else {
        return;
    };

    let (rotate, opposite) = match (first.left, second.left) {
        (true, false) => (first, second),
        (false, true) => (second, first),
        _ => return,
    };

    let Some(width) = unsigned_width(rotate.value) else {
        return;
    };

    if rotate.amount == 0
        || rotate.amount >= width
        || rotate.amount.checked_add(opposite.amount) != Some(width)
    {
        return;
    }

    if !expressions_equivalent(rotate.value, opposite.value) || !is_side_effect_free(rotate.value) {
        return;
    }

    ctx.sink.push(diagnostics::lint::manual_rotate(span, width));
}

struct Shift<'a> {
    value: &'a Expression,
    amount: u64,
    left: bool,
}

fn as_shift(expression: &Expression) -> Option<Shift<'_>> {
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };

    let left_shift = match operator {
        BinaryOperator::ShiftLeft => true,
        BinaryOperator::ShiftRight => false,
        _ => return None,
    };

    Some(Shift {
        value: left.unwrap_parens(),
        amount: right.as_integer()?,
        left: left_shift,
    })
}

fn unsigned_width(value: &Expression) -> Option<u64> {
    match value.get_type().as_simple()? {
        SimpleKind::Uint8 | SimpleKind::Byte => Some(8),
        SimpleKind::Uint16 => Some(16),
        SimpleKind::Uint32 => Some(32),
        SimpleKind::Uint64 => Some(64),
        _ => None,
    }
}
