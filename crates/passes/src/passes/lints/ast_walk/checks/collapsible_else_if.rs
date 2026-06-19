use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Span};

pub fn check_collapsible_else_if(expression: &Expression, ctx: &NodeCtx) {
    let Expression::If {
        consequence,
        alternative,
        ..
    } = expression
    else {
        return;
    };

    if consequence.diverges().is_some() {
        return;
    }

    let Expression::Block { items, .. } = alternative.as_ref() else {
        return;
    };
    let [
        Expression::If {
            span: inner_span, ..
        },
    ] = items.as_slice()
    else {
        return;
    };

    let inner_if_keyword_span = Span::new(inner_span.file_id, inner_span.byte_offset, 2);
    ctx.sink.push(diagnostics::lint::collapsible_else_if(
        &inner_if_keyword_span,
    ));
}
