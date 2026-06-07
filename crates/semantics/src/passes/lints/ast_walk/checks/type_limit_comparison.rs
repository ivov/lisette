use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::SimpleKind;

use super::helpers::{flip_comparison, signed_integer_literal};

pub fn check_type_limit_comparison(expression: &Expression, ctx: &NodeCtx) {
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
    if !matches!(
        operator,
        LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    ) {
        return;
    }

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();

    let (value, bound, operator) =
        match (signed_integer_literal(left), signed_integer_literal(right)) {
            (None, Some(bound)) => (left, bound, *operator),
            (Some(bound), None) => (right, bound, flip_comparison(*operator)),
            _ => return,
        };

    let Some((floor, ceil)) = fixed_width_bounds(value.get_type().underlying_simple_kind()) else {
        return;
    };

    let always_true = match operator {
        GreaterThan if bound == ceil => false,
        LessThanOrEqual if bound == ceil => true,
        LessThan if Some(bound) == floor => false,
        GreaterThanOrEqual if Some(bound) == floor => true,
        _ => return,
    };

    ctx.sink
        .push(diagnostics::lint::type_limit_comparison(span, always_true));
}

// The representable limits of a fixed-width integer type as `(floor, ceil)`. The
// floor is `None` for unsigned types: their `x < 0` / `x >= 0` floor is owned by
// `unsigned_comparison` (which also handles the platform-width `uint` this never
// sees), so only the ceiling is reported here for unsigned types.
fn fixed_width_bounds(kind: Option<SimpleKind>) -> Option<(Option<i128>, i128)> {
    use SimpleKind::*;
    let (floor, ceil) = match kind? {
        Int8 => (Some(i8::MIN as i128), i8::MAX as i128),
        Int16 => (Some(i16::MIN as i128), i16::MAX as i128),
        Int32 => (Some(i32::MIN as i128), i32::MAX as i128),
        Int64 => (Some(i64::MIN as i128), i64::MAX as i128),
        Uint8 | Byte => (None, u8::MAX as i128),
        Uint16 => (None, u16::MAX as i128),
        Uint32 => (None, u32::MAX as i128),
        Uint64 => (None, u64::MAX as i128),
        _ => return None,
    };
    Some((floor, ceil))
}
