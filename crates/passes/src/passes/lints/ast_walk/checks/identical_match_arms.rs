use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, MatchOrigin, Span};

use syntax::ast::collect_pattern_bindings;

use super::helpers::{expressions_equivalent, is_empty_block};

pub fn check_identical_match_arms(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match {
        subject,
        arms,
        origin,
        span,
        ..
    } = expression
    else {
        return;
    };

    if !matches!(origin, MatchOrigin::Explicit) {
        return;
    }

    if arms.len() < 2 {
        return;
    }

    if !is_safe_to_drop(subject) {
        return;
    }

    // A bound name ties a body to its own arm; a guard makes the arm conditional.
    if arms
        .iter()
        .any(|arm| arm.has_guard() || !collect_pattern_bindings(&arm.pattern).is_empty())
    {
        return;
    }

    let first = &arms[0].expression;

    // Empty blocks are in-progress stubs; `empty_match_arm` covers them.
    if is_empty_block(first) {
        return;
    }

    if !arms[1..]
        .iter()
        .all(|arm| expressions_equivalent(first, &arm.expression))
    {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink
        .push(diagnostics::lint::identical_match_arms(&match_keyword_span));
}

fn is_safe_to_drop(expression: &Expression) -> bool {
    let expression = expression.unwrap_parens();
    let node_is_safe = match expression {
        Expression::Identifier { .. }
        | Expression::Literal { .. }
        | Expression::Unary { .. }
        | Expression::DotAccess { .. } => true,
        Expression::Binary { operator, .. } => !matches!(
            operator,
            BinaryOperator::Division
                | BinaryOperator::Remainder
                | BinaryOperator::ShiftLeft
                | BinaryOperator::ShiftRight
        ),
        _ => false,
    };

    node_is_safe && expression.children().into_iter().all(is_safe_to_drop)
}
