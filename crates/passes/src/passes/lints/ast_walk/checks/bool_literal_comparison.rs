use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::bool_literal;

pub fn check_bool_literal_comparison(expression: &Expression, ctx: &NodeCtx) {
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
    let is_equal = match operator {
        Equal => true,
        NotEqual => false,
        _ => return,
    };

    // Pick the non-literal operand; bail on `true == false` (lit vs lit) since
    // check_self_comparison and const-folding are more appropriate there.
    let (other, bool_value) = match (
        bool_literal(left.unwrap_parens()),
        bool_literal(right.unwrap_parens()),
    ) {
        (Some(b), None) => (right.unwrap_parens(), b),
        (None, Some(b)) => (left.unwrap_parens(), b),
        _ => return,
    };

    // Skip operands that cannot be rendered as a dotted path — suggesting `!x`
    // for `f() == true` would be misleading since no `x` exists.
    let Some(other_text) = other.as_dotted_path() else {
        return;
    };

    let negate = bool_value != is_equal;
    let replacement = if negate {
        format!("!{other_text}")
    } else {
        other_text
    };

    ctx.sink.push(diagnostics::lint::bool_literal_comparison(
        span,
        &replacement,
    ));
}
