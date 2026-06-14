use crate::passes::comparison::{MaskComparison, MaskOp, mask_comparison};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

pub fn check_ineffective_bit_mask(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary { span, .. } = expression else {
        return;
    };
    let Some(mask) = mask_comparison(expression) else {
        return;
    };
    if mask.mask_op != MaskOp::Or || !is_ineffective(&mask) {
        return;
    }

    ctx.sink.push(diagnostics::lint::ineffective_bit_mask(
        span,
        "|",
        mask.mask,
        mask.constant,
    ));
}

/// A positive `x | m` sets only bits below a power-of-two boundary, so comparing
/// it there gives the same answer as comparing `x` directly.
fn is_ineffective(mask: &MaskComparison) -> bool {
    use BinaryOperator::*;
    if mask.mask < 1 {
        return false;
    }
    let constant = mask.constant;
    match mask.operator {
        LessThan | GreaterThanOrEqual => is_power_of_two(constant) && mask.mask < constant,
        LessThanOrEqual | GreaterThan => is_power_of_two(constant + 1) && mask.mask <= constant,
        _ => false,
    }
}

fn is_power_of_two(value: i128) -> bool {
    value > 0 && value & (value - 1) == 0
}
