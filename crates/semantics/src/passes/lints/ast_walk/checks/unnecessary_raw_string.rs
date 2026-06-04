use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal, Pattern};

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
        ctx.sink
            .push(diagnostics::lint::unnecessary_raw_string(span));
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
        ctx.sink
            .push(diagnostics::lint::unnecessary_raw_string(span));
    }
}
