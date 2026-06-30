use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};

use super::helpers::span_text;

pub fn check_negated_equality(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Unary {
        operator: UnaryOperator::Not,
        expression: operand,
        span,
        ..
    } = expression
    else {
        return;
    };

    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = operand.unwrap_parens()
    else {
        return;
    };

    let (is_equal, flipped) = match operator {
        BinaryOperator::Equal => (true, "!="),
        BinaryOperator::NotEqual => (false, "=="),
        _ => return,
    };

    let mut diagnostic = diagnostics::lint::negated_equality(span, is_equal);

    if let (Some(lhs), Some(rhs)) = (span_text(ctx.source, left), span_text(ctx.source, right)) {
        let replacement = format!("{lhs} {flipped} {rhs}");
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }

    ctx.sink.push(diagnostic);
}
