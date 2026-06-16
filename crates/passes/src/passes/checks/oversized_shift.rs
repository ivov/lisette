use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::SimpleKind;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
        && matches!(
            operator,
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
        )
        && let Some(kind) = left.get_type().as_simple()
        && let Some(bit_width) = fixed_bit_width(kind)
        && let Some(value) = right.as_integer()
        && value >= u64::from(bit_width)
    {
        ctx.sink.push(diagnostics::infer::oversized_shift(
            span,
            kind.leaf_name(),
            bit_width,
            value,
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
