use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Literal, Pattern, Span};

pub fn check_unnecessary_raw_string_expression(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Literal {
        literal: Literal::String { value, raw: true },
        span,
        ..
    } = expression
    else {
        return;
    };
    if !value.contains('\\') {
        ctx.sink.push(
            diagnostics::lint::unnecessary_raw_string(span).with_fix(Fix::new(
                "Remove the `r` prefix",
                Edit::deletion(Span::new(span.file_id, span.byte_offset, 1)),
            )),
        );
    }
}

pub fn check_unnecessary_raw_string_pattern(pattern: &Pattern, ctx: &NodeCtx) {
    let Pattern::Literal {
        literal: Literal::String { value, raw: true },
        span,
        ..
    } = pattern
    else {
        return;
    };
    if !value.contains('\\') {
        ctx.sink.push(
            diagnostics::lint::unnecessary_raw_string(span).with_fix(Fix::new(
                "Remove the `r` prefix",
                Edit::deletion(Span::new(span.file_id, span.byte_offset, 1)),
            )),
        );
    }
}
