use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Constant, Expression};
use syntax::types::SimpleKind;

use super::constant_overflow::claim_constant;

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
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

    if !matches!(
        operator,
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
    ) {
        return;
    }

    let Some(count) = right.fold_constant() else {
        return;
    };

    // Go converts the count to `uint`, not to the type of the left operand.
    claim_constant(right, ctx);

    let Constant::Integer(amount) = count else {
        return;
    };

    if amount < 0 {
        ctx.sink
            .push(diagnostics::infer::negative_shift(span, amount));
        return;
    }

    if amount > i128::from(u64::MAX) {
        ctx.sink
            .push(diagnostics::infer::shift_amount_too_large(span, amount));
        return;
    }

    if let Some(kind) = left.get_type().as_simple()
        && let Some(bit_width) = fixed_bit_width(kind)
        && amount >= i128::from(bit_width)
    {
        ctx.sink.push(diagnostics::infer::oversized_shift(
            span,
            kind.leaf_name(),
            bit_width,
            amount,
        ));
    }
}

fn fixed_bit_width(kind: SimpleKind) -> Option<u32> {
    match kind {
        SimpleKind::Int8 | SimpleKind::Uint8 | SimpleKind::Byte => Some(8),
        SimpleKind::Int16 | SimpleKind::Uint16 => Some(16),
        SimpleKind::Int32 | SimpleKind::Uint32 | SimpleKind::Rune => Some(32),
        SimpleKind::Int64 | SimpleKind::Uint64 => Some(64),
        _ => None,
    }
}
