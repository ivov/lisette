use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Span, UnaryOperator};

use super::helpers::span_text;

pub fn check_double_negation(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Unary {
        operator,
        expression: operand,
        span: outer_span,
        ..
    } = expression
    else {
        return;
    };

    let Expression::Unary {
        operator: inner_op,
        expression: inner_operand,
        span: inner_span,
        ..
    } = operand.unwrap_parens()
    else {
        return;
    };

    if operator != inner_op {
        return;
    }

    if !matches!(operator, UnaryOperator::Not | UnaryOperator::Negative) {
        return;
    }

    let operators_span = Span::new(
        outer_span.file_id,
        outer_span.byte_offset,
        inner_span.byte_offset - outer_span.byte_offset + 1,
    );

    let is_bool = *operator == UnaryOperator::Not;
    let mut diagnostic = diagnostics::lint::double_negation(&operators_span, is_bool);

    if let Some(text) = span_text(ctx.source, inner_operand) {
        diagnostic = diagnostic.with_fix(Fix::new(
            "Remove double negation",
            Edit::replacement(*outer_span, text),
        ));
    }

    ctx.sink.push(diagnostic);
}
