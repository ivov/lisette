use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::{as_tight_operand, bool_literal, negate_comparison, span_text};

pub fn check_unnecessary_bool(expression: &Expression, ctx: &NodeCtx) {
    let Expression::If {
        condition,
        consequence,
        alternative,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (Some(then_value), Some(else_value)) = (
        block_single_bool(consequence),
        block_single_bool(alternative),
    ) else {
        return;
    };

    // Equal literals are `identical_if_branches`, not a condition rewrite.
    if then_value == else_value {
        return;
    }

    let condition = condition.unwrap_parens();
    // A bool newtype (`Flag(bool)`) result or condition is not plain-bool redundancy.
    if !expression.get_type().is_boolean() || !condition.get_type().is_boolean() {
        return;
    }

    let mut diagnostic = diagnostics::lint::unnecessary_bool(span, then_value);
    let replacement = if then_value {
        span_text(ctx.source, condition).map(|text| as_tight_operand(text, condition))
    } else {
        negated_condition(ctx.source, condition)
    };
    if let Some(replacement) = replacement {
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}

/// Source text negating `condition`: flip a comparison operator, else prefix `!`.
fn negated_condition(source: &str, condition: &Expression) -> Option<String> {
    if let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = condition
        && let Some(negated) = negate_comparison(*operator)
        && flip_preserves_nan(*operator, left, right)
    {
        let lhs = span_text(source, left)?;
        let rhs = span_text(source, right)?;
        return Some(format!("({lhs} {negated} {rhs})"));
    }
    let text = span_text(source, condition)?;
    Some(format!("(!{})", as_tight_operand(text, condition)))
}

// Ordered flips are not NaN-safe (`!(a < b)` is not `a >= b`), only `==`/`!=` are.
fn flip_preserves_nan(operator: BinaryOperator, left: &Expression, right: &Expression) -> bool {
    matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
        || (is_non_float(left) && is_non_float(right))
}

fn is_non_float(expression: &Expression) -> bool {
    expression
        .get_type()
        .underlying_simple_kind()
        .is_some_and(|kind| !kind.is_float())
}

fn block_single_bool(expression: &Expression) -> Option<bool> {
    let Expression::Block { items, .. } = expression else {
        return None;
    };
    let [only] = items.as_slice() else {
        return None;
    };
    bool_literal(only.unwrap_parens())
}
