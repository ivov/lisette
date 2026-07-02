use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Pattern};

use super::helpers::span_text;

pub fn check_let_and_return(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Block { items, .. } = expression else {
        return;
    };

    let [.., binding_statement, tail] = items.as_slice() else {
        return;
    };

    let Expression::Let {
        binding,
        value: bound_value,
        mutable: false,
        else_block: None,
        span,
        ..
    } = binding_statement
    else {
        return;
    };

    // An annotation can coerce the value's type (for example binding a concrete
    // type to an interface), which the tail position would otherwise lose.
    if binding.annotation.is_some() {
        return;
    }

    let Pattern::Identifier { identifier, .. } = &binding.pattern else {
        return;
    };

    let Expression::Identifier { value, .. } = tail else {
        return;
    };

    if value != identifier {
        return;
    }

    let mut diagnostic = diagnostics::lint::let_and_return(span);
    if let Some(value_text) = span_text(ctx.source, bound_value) {
        let edit_span = span.merge(tail.get_span());
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{value_text}`"),
            Edit::replacement(edit_span, value_text.to_string()),
        ));
    }
    ctx.sink.push(diagnostic);
}
