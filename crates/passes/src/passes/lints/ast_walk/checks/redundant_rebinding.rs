use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Pattern};

/// Flags `let x = x`, an immutable rebinding of a variable to itself.
pub fn check_redundant_rebinding(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Let {
        binding,
        value,
        mutable: false,
        else_block: None,
        span,
        ..
    } = expression
    else {
        return;
    };

    // An annotation can coerce the value (for example to an interface).
    if binding.annotation.is_some() {
        return;
    }

    let Pattern::Identifier {
        identifier,
        span: new_span,
    } = &binding.pattern
    else {
        return;
    };

    let Expression::Identifier {
        value: rhs_name,
        binding_id: Some(outer_id),
        ..
    } = value.unwrap_parens()
    else {
        return;
    };

    if rhs_name != identifier {
        return;
    }

    // `let x = x` copies into a distinct storage slot, so a `&x` on either
    // binding (recorded as `mutated`) makes the two slots observable apart
    // through a `Ref` and removing the rebinding would merge them. A `mut`
    // outer is a deliberate freeze; an unused new binding is owned by
    // `unused_variable`.
    let outer_is_stable = ctx
        .facts
        .bindings
        .get(outer_id)
        .is_some_and(|b| !b.kind.is_mutable() && !b.mutated);
    if !outer_is_stable {
        return;
    }

    let new_is_plain_use = ctx
        .facts
        .bindings
        .values()
        .any(|b| b.span == *new_span && b.used && !b.mutated);
    if !new_is_plain_use {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::redundant_rebinding(span, identifier));
}
