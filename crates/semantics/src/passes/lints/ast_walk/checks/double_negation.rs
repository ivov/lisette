use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Span, UnaryOperator};

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
    ctx.sink
        .push(diagnostics::lint::double_negation(&operators_span, is_bool));
}
