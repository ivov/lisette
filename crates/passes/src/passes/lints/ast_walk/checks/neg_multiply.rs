use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};

use super::helpers::{signed_integer_literal, span_text};

pub fn check_neg_multiply(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator: BinaryOperator::Multiplication,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    // Skip the lowered `a *= -1` (operator `*=`), whose span is the assignment's,
    // so a `-a` suggestion would drop the assignment. A genuine `a * -1` has no `=`.
    let gap = (left.get_span().end() as usize)..(right.get_span().byte_offset as usize);
    if ctx.source.get(gap).is_some_and(|text| text.contains('=')) {
        return;
    }

    let operand = if signed_integer_literal(right.unwrap_parens()) == Some(-1) {
        left
    } else if signed_integer_literal(left.unwrap_parens()) == Some(-1) {
        right
    } else {
        return;
    };

    // Exclude `-1 * -1` (a constant fold) and `-x * -1` (a degenerate `--x`).
    let inner = operand.unwrap_parens();
    if signed_integer_literal(inner) == Some(-1)
        || matches!(
            inner,
            Expression::Unary {
                operator: UnaryOperator::Negative,
                ..
            }
        )
    {
        return;
    }

    // `-operand` is only well-typed (and only checker-accepted) for signed or float.
    if !inner
        .get_type()
        .underlying_simple_kind()
        .is_some_and(|kind| kind.is_signed_int() || kind.is_float())
    {
        return;
    }

    let Some(text) = span_text(ctx.source, operand) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::neg_multiply(span, text));
}
