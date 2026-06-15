use crate::passes::comparison::{MinMaxOp, prelude_min_max};
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;
use syntax::types::SimpleKind;

use super::helpers::{expressions_equivalent, is_side_effect_free, signed_integer_literal};

pub fn check_unnecessary_min_or_max(expression: &Expression, ctx: &NodeCtx) {
    let Some(call) = prelude_min_max(expression) else {
        return;
    };

    if expression.get_type().is_error() {
        return;
    }

    let op = match call.op {
        MinMaxOp::Min => "min",
        MinMaxOp::Max => "max",
    };

    if expressions_equivalent(call.left, call.right) && is_side_effect_free(call.left) {
        ctx.sink.push(diagnostics::lint::unnecessary_min_or_max(
            &expression.get_span(),
            op,
        ));
        return;
    }

    let Some((value_operand, literal)) = split_one_literal(call.left, call.right) else {
        return;
    };
    let Some(kind) = value_operand.get_type().as_simple() else {
        return;
    };
    let no_op_bound = match call.op {
        MinMaxOp::Min => max_bound(kind),
        MinMaxOp::Max => min_bound(kind),
    };
    if no_op_bound == Some(literal) {
        ctx.sink.push(diagnostics::lint::unnecessary_min_or_max(
            &expression.get_span(),
            op,
        ));
    }
}

fn split_one_literal<'a>(a: &'a Expression, b: &'a Expression) -> Option<(&'a Expression, i128)> {
    match (
        signed_integer_literal(a.unwrap_parens()),
        signed_integer_literal(b.unwrap_parens()),
    ) {
        (None, Some(value)) => Some((a, value)),
        (Some(value), None) => Some((b, value)),
        _ => None,
    }
}

fn min_bound(kind: SimpleKind) -> Option<i128> {
    use SimpleKind::*;
    Some(match kind {
        Int8 => i8::MIN as i128,
        Int16 => i16::MIN as i128,
        Int32 | Rune => i32::MIN as i128,
        Int64 => i64::MIN as i128,
        Uint8 | Byte | Uint16 | Uint32 | Uint64 | Uint | Uintptr => 0,
        _ => return None,
    })
}

fn max_bound(kind: SimpleKind) -> Option<i128> {
    use SimpleKind::*;
    Some(match kind {
        Int8 => i8::MAX as i128,
        Int16 => i16::MAX as i128,
        Int32 | Rune => i32::MAX as i128,
        Int64 => i64::MAX as i128,
        Uint8 | Byte => u8::MAX as i128,
        Uint16 => u16::MAX as i128,
        Uint32 => u32::MAX as i128,
        Uint64 => u64::MAX as i128,
        _ => return None,
    })
}
