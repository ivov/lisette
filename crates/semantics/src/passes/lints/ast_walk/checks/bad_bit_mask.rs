use crate::passes::comparison::{MaskComparison, MaskOp, mask_comparison};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

pub fn check_bad_bit_mask(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary { span, .. } = expression else {
        return;
    };
    let Some(mask) = mask_comparison(expression) else {
        return;
    };
    let Some(always_true) = constant_result(&mask) else {
        return;
    };
    ctx.sink
        .push(diagnostics::lint::bad_bit_mask(span, always_true));
}

fn constant_result(mask: &MaskComparison) -> Option<bool> {
    use BinaryOperator::*;
    match mask.operator {
        Equal | NotEqual => equality_result(mask),
        LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual => relational_result(mask),
        _ => None,
    }
}

fn equality_result(mask: &MaskComparison) -> Option<bool> {
    let reachable = match mask.mask_op {
        MaskOp::And => (mask.constant & mask.mask) == mask.constant,
        MaskOp::Or => (mask.constant | mask.mask) == mask.constant,
    };
    if reachable {
        return None;
    }
    Some(matches!(mask.operator, BinaryOperator::NotEqual))
}

/// Skips cases already constant over the full type range, leaving those to
/// `type_limit_comparison`/`unsigned_comparison`.
fn relational_result(mask: &MaskComparison) -> Option<bool> {
    let (low, high) = mask_range(mask)?;
    let constant = relational_over(low, high, mask.operator, mask.constant)?;
    let (type_low, type_high) = mask.kind.integer_range()?;
    if relational_over(type_low, type_high, mask.operator, mask.constant).is_some() {
        return None;
    }
    Some(constant)
}

/// `x & m` is `[0, m]` for any sign; `x | m` is `[m, max]` only for unsigned `x`
/// (a negative `x` drops below `m`). Negative masks have no useful interval.
fn mask_range(mask: &MaskComparison) -> Option<(i128, i128)> {
    if mask.mask < 1 {
        return None;
    }
    match mask.mask_op {
        MaskOp::And => Some((0, mask.mask)),
        MaskOp::Or => {
            if !mask.kind.is_unsigned_int() {
                return None;
            }
            let (_, max) = mask.kind.integer_range()?;
            Some((mask.mask, max))
        }
    }
}

fn relational_over(
    low: i128,
    high: i128,
    operator: BinaryOperator,
    constant: i128,
) -> Option<bool> {
    let at_low = relational_holds(low, operator, constant);
    if at_low == relational_holds(high, operator, constant) {
        Some(at_low)
    } else {
        None
    }
}

fn relational_holds(value: i128, operator: BinaryOperator, constant: i128) -> bool {
    use BinaryOperator::*;
    match operator {
        LessThan => value < constant,
        LessThanOrEqual => value <= constant,
        GreaterThan => value > constant,
        GreaterThanOrEqual => value >= constant,
        _ => false,
    }
}
