use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Span};

use super::helpers::replacement_drops_comment;

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
    let mut diagnostic = diagnostics::lint::collapsible_else_if(&inner_if_keyword_span);
    let alternative_span = alternative.get_span();
    if let Some(inner_text) = ctx
        .source
        .get(inner_span.byte_offset as usize..inner_span.end() as usize)
        && !replacement_drops_comment(ctx.source, alternative_span, inner_text)
    {
        diagnostic = diagnostic.with_fix(Fix::new(
            "Collapse into `else if`",
            Edit::replacement(alternative_span, inner_text.to_string()),
        ));
    }
    ctx.sink.push(diagnostic);
}
