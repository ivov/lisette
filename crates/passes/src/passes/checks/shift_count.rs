use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::constant::MAX_CONSTANT_SHIFT;
use syntax::types::SimpleKind;

use super::constant_overflow::claim_constant;

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
    let (operator, left, right, span) = match expression {
        Expression::Binary {
            operator,
            left,
            right,
            span,
            ..
        } => (operator, left, right, span),
        Expression::Assignment {
            target,
            compound_operator: Some(operator),
            value,
            span,
            ..
        } => (operator, target, value, span),
        _ => return,
    };

    if !matches!(
        operator,
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
    ) {
        return;
    }

    let Ok(Some(count)) = ctx.evaluate(right) else {
        return;
    };

    // Go converts the count to `uint`, not to the type of the left operand.
    claim_constant(right, ctx);

    let Some(amount) = count.value.as_integer() else {
        return;
    };
    let amount_text = amount.to_string();

    if amount < 0 {
        ctx.sink
            .push(diagnostics::infer::negative_shift(span, &amount_text));
        return;
    }

    if amount > i128::from(u64::MAX) {
        ctx.sink.push(diagnostics::infer::shift_amount_too_large(
            span,
            &amount_text,
        ));
        return;
    }

    if !matches!(ctx.evaluate(left), Ok(None)) {
        if amount > i128::from(MAX_CONSTANT_SHIFT) {
            ctx.sink.push(diagnostics::infer::constant_shift_too_large(
                span,
                &amount_text,
                MAX_CONSTANT_SHIFT,
            ));
        }
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
            &amount_text,
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
