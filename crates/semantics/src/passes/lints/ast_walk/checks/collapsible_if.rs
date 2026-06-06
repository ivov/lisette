use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Span};

pub fn check_collapsible_if(expression: &Expression, ctx: &NodeCtx) {
    let Expression::If {
        condition,
        consequence,
        alternative,
        ..
    } = expression
    else {
        return;
    };

    if !is_missing_else(alternative) {
        return;
    }

    let Expression::Block { items, .. } = consequence.as_ref() else {
        return;
    };
    let [
        Expression::If {
            condition: inner_condition,
            alternative: inner_alternative,
            span: inner_span,
            ..
        },
    ] = items.as_slice()
    else {
        return;
    };
    if !is_missing_else(inner_alternative) {
        return;
    }

    if !condition.get_type().is_boolean() || !inner_condition.get_type().is_boolean() {
        return;
    }

    let inner_if_keyword_span = Span::new(inner_span.file_id, inner_span.byte_offset, 2);
    ctx.sink
        .push(diagnostics::lint::collapsible_if(&inner_if_keyword_span));
}

fn is_missing_else(alternative: &Expression) -> bool {
    matches!(alternative, Expression::Unit { .. })
}
