use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Pattern};

pub fn check_let_and_return(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Block { items, .. } = expression else {
        return;
    };

    let [.., binding_statement, tail] = items.as_slice() else {
        return;
    };

    let Expression::Let {
        binding,
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

    ctx.sink.push(diagnostics::lint::let_and_return(span));
}
