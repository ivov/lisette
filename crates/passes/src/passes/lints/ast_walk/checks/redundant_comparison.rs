use crate::passes::comparison::{
    Bound, flip_comparison, in_scope_comparison, is_side_effect_free, signed_integer_literal,
    tighter,
};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::expressions_equivalent;

/// Inspects a single `&&`/`||` of two directly-joined comparisons; flattened
/// chains and swapped-operand forms are out of scope.
pub fn check_redundant_comparison(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = expression
    else {
        return;
    };

    use BinaryOperator::*;
    if !matches!(operator, And | Or) {
        return;
    }

    let Some((left_operand, left_interval)) = comparison_interval(left) else {
        return;
    };
    let Some((right_operand, right_interval)) = comparison_interval(right) else {
        return;
    };

    if !is_side_effect_free(left_operand) || !expressions_equivalent(left_operand, right_operand) {
        return;
    }

    let left_subset = is_subset(left_interval, right_interval);
    let right_subset = is_subset(right_interval, left_interval);
    // Equal bounds are a duplicate, left to `duplicate_logical_operand`; overlap
    // and disjoint pairs are not redundant.
    if left_subset == right_subset {
        return;
    }

    let redundant_span = match operator {
        // `||` keeps the wider bound, so the narrower (subset) side is redundant.
        Or if left_subset => left.get_span(),
        Or => right.get_span(),
        // `&&` keeps the narrower bound, so the wider (superset) side is redundant.
        And if left_subset => right.get_span(),
        And => left.get_span(),
        _ => return,
    };

    ctx.sink
        .push(diagnostics::lint::redundant_comparison(&redundant_span));
}

#[derive(Clone, Copy)]
struct Interval {
    low: Bound,
    high: Bound,
}

/// The interval a `variable OP literal` comparison constrains its operand to,
/// paired with that operand. `None` for anything that is not such a comparison.
fn comparison_interval(expression: &Expression) -> Option<(&Expression, Interval)> {
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };

    use BinaryOperator::*;
    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    if !in_scope_comparison(left, right) {
        return None;
    }
    let (operand, operator, bound) =
        match (signed_integer_literal(left), signed_integer_literal(right)) {
            (None, Some(bound)) => (left, *operator, bound),
            (Some(bound), None) => (right, flip_comparison(*operator), bound),
            _ => return None,
        };

    let interval = match operator {
        LessThan => Interval {
            low: None,
            high: Some((bound, false)),
        },
        LessThanOrEqual => Interval {
            low: None,
            high: Some((bound, true)),
        },
        GreaterThan => Interval {
            low: Some((bound, false)),
            high: None,
        },
        GreaterThanOrEqual => Interval {
            low: Some((bound, true)),
            high: None,
        },
        Equal => Interval {
            low: Some((bound, true)),
            high: Some((bound, true)),
        },
        _ => return None,
    };

    Some((operand, interval))
}

/// True when every value allowed by `a` is also allowed by `b`.
fn is_subset(a: Interval, b: Interval) -> bool {
    tighter(a.low, b.low, |x, y| x > y) == a.low && tighter(a.high, b.high, |x, y| x < y) == a.high
}
