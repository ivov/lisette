use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::{bool_literal, expressions_equivalent, span_text};

pub fn check_needless_bool_assign(expression: &Expression, ctx: &NodeCtx) {
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

    let (Some((then_target, then_value)), Some((else_target, else_value))) = (
        single_bool_assignment(consequence),
        single_bool_assignment(alternative),
    ) else {
        return;
    };

    // Equal values are `identical_if_branches`, not a condition rewrite.
    if then_value == else_value {
        return;
    }

    // Only a bare identifier target is order-safe. A field or deref target
    // (`obj.flag`, `p.*`) evaluates its base before the right-hand side, so the
    // rewrite `target = cond()` could capture a different location than the
    // branch would if `cond()` mutates that base.
    if !matches!(then_target, Expression::Identifier { .. })
        || !expressions_equivalent(then_target, else_target)
    {
        return;
    }

    let condition = condition.unwrap_parens();
    let (Some(target), Some(condition_text)) = (
        span_text(ctx.source, then_target),
        span_text(ctx.source, condition),
    ) else {
        return;
    };

    let negate = !then_value;
    let replacement = if negate {
        format!("{target} = !({condition_text})")
    } else {
        format!("{target} = {condition_text}")
    };

    ctx.sink.push(
        diagnostics::lint::needless_bool_assign(span, target, condition_text, negate).with_fix(
            Fix::new(
                format!("Replace with `{replacement}`"),
                Edit::replacement(*span, replacement.clone()),
            ),
        ),
    );
}

fn single_bool_assignment(block: &Expression) -> Option<(&Expression, bool)> {
    let Expression::Block { items, .. } = block else {
        return None;
    };
    let [
        Expression::Assignment {
            target,
            value,
            compound_operator: None,
            ..
        },
    ] = items.as_slice()
    else {
        return None;
    };
    Some((target.unwrap_parens(), bool_literal(value.unwrap_parens())?))
}
