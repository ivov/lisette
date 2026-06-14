use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::{SimpleKind, Type};

use super::helpers::{expressions_equivalent, is_side_effect_free, signed_integer_literal};

pub fn check_equal_operands(expression: &Expression, ctx: &NodeCtx) {
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
    let note = match operator {
        BitwiseAnd | BitwiseOr => "is that operand",
        BitwiseXor | BitwiseAndNot | Subtraction => "is always `0`",
        Division => "is always `1`, unless the operand is `0`, which panics",
        Remainder => "is always `0`, unless the operand is `0`, which panics",
        _ => return,
    };
    let bitwise = matches!(
        operator,
        BitwiseAnd | BitwiseOr | BitwiseXor | BitwiseAndNot
    );

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    if !expressions_equivalent(left, right) {
        return;
    }
    // Skip literal folds and side-effecting operands, which may differ per eval.
    if signed_integer_literal(left).is_some() || !is_side_effect_free(left) {
        return;
    }
    if !is_integer_operand(&left.get_type(), bitwise) {
        return;
    }

    ctx.sink.push(diagnostics::lint::equal_operands(span, note));
}

/// Bitwise ops also accept a direct `uintptr` (per the checker's
/// `is_integer_type`), but not a `uintptr` alias/newtype, so that exception is
/// gated on `as_simple` rather than the underlying kind.
fn is_integer_operand(ty: &Type, bitwise: bool) -> bool {
    if ty
        .underlying_simple_kind()
        .is_some_and(|kind| kind.is_signed_int() || kind.is_unsigned_int())
    {
        return true;
    }
    bitwise && ty.as_simple() == Some(SimpleKind::Uintptr)
}
