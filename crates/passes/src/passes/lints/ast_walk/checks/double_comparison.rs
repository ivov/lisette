use crate::passes::comparison::{in_scope_comparison, is_side_effect_free};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::expressions_equivalent;

/// Inspects a single `&&`/`||` of two directly-joined comparisons over the same
/// operands; flattened chains and swapped-operand forms are out of scope.
pub fn check_double_comparison(expression: &Expression, ctx: &NodeCtx) {
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
    if !matches!(operator, And | Or) {
        return;
    }

    let Some((left_op, left_lhs, left_rhs)) = as_comparison(left) else {
        return;
    };
    let Some((right_op, right_lhs, right_rhs)) = as_comparison(right) else {
        return;
    };

    if !(expressions_equivalent(left_lhs, right_lhs) && expressions_equivalent(left_rhs, right_rhs))
    {
        return;
    }
    if !(is_side_effect_free(left_lhs) && is_side_effect_free(left_rhs)) {
        return;
    }

    let both_non_float = is_known_non_float(left_lhs) && is_known_non_float(left_rhs);
    let Some(combined) = combine(*operator, left_op, right_op, both_non_float) else {
        return;
    };

    ctx.sink
        .push(diagnostics::lint::double_comparison(span, combined));
}

/// True only when the operand's type is known to have a non-float underlying
/// kind. An unknown kind (e.g. an unbounded type parameter) is possibly-float.
fn is_known_non_float(expression: &Expression) -> bool {
    expression
        .get_type()
        .underlying_simple_kind()
        .is_some_and(|kind| !kind.is_float())
}

fn as_comparison(expression: &Expression) -> Option<(BinaryOperator, &Expression, &Expression)> {
    use BinaryOperator::*;
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    if !matches!(
        operator,
        LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual | Equal | NotEqual
    ) {
        return None;
    }
    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    if !in_scope_comparison(left, right) {
        return None;
    }
    Some((*operator, left, right))
}

/// The single operator two comparisons over the same operands collapse into, if
/// the pair is simplifiable. The `< || > => !=` case needs both operands known
/// non-float: NaN makes both orderings false yet `!=` true, so it differs for floats.
fn combine(
    outer: BinaryOperator,
    a: BinaryOperator,
    b: BinaryOperator,
    both_non_float: bool,
) -> Option<&'static str> {
    use BinaryOperator::*;
    let pair = |x, y| (a == x && b == y) || (a == y && b == x);
    match outer {
        Or if pair(LessThan, Equal) => Some("<="),
        Or if pair(GreaterThan, Equal) => Some(">="),
        Or if pair(LessThan, GreaterThan) && both_non_float => Some("!="),
        And if pair(LessThanOrEqual, GreaterThanOrEqual) => Some("=="),
        _ => None,
    }
}
