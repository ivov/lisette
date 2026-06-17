use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, FormatStringPart, Literal};

pub fn check_uninterpolated_fstring(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Literal {
        literal: Literal::FormatString(parts),
        span,
        ..
    } = expression
    else {
        return;
    };

    let has_interpolation = parts
        .iter()
        .any(|p| matches!(p, FormatStringPart::Expression(_)));

    if !has_interpolation {
        ctx.sink
            .push(diagnostics::lint::uninterpolated_fstring(span));
    }
}
