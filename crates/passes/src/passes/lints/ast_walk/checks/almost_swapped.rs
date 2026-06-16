use crate::passes::walk::NodeCtx;
use syntax::ast::{BindingId, Expression};
use syntax::types::Type;

pub fn check_almost_swapped(expression: &Expression, ctx: &NodeCtx) {
    let (Expression::Block { items, .. }
    | Expression::TryBlock { items, .. }
    | Expression::RecoverBlock { items, .. }) = expression
    else {
        return;
    };

    let mut index = 0;
    while index + 1 < items.len() {
        if fire_if_swapped(&items[index], &items[index + 1], ctx) {
            index += 2;
        } else {
            index += 1;
        }
    }
}

fn fire_if_swapped(first: &Expression, second: &Expression, ctx: &NodeCtx) -> bool {
    let Some((first_target, first_value)) = plain_assignment(first) else {
        return false;
    };
    let Some((second_target, second_value)) = plain_assignment(second) else {
        return false;
    };

    let (Some(left), Some(right)) = (variable(first_target), variable(first_value)) else {
        return false;
    };
    let (Some(second_to), Some(second_from)) = (variable(second_target), variable(second_value))
    else {
        return false;
    };

    // The second statement must reassign two distinct variables in the opposite
    // direction: `a = b; b = a`.
    if left.id != second_from.id || right.id != second_to.id || left.id == right.id {
        return false;
    }

    // Both targets must be declared mutable, so the checker accepts both
    // assignments rather than reporting an immutable-mutation error.
    if !is_mutable(left.id, ctx) || !is_mutable(right.id, ctx) {
        return false;
    }

    // Same type, so both assignments type-check; a mismatch or an `Error` operand
    // fails this check and the checker owns the diagnostic.
    if left.ty != right.ty {
        return false;
    }

    let span = first.get_span().merge(second.get_span());
    ctx.sink.push(diagnostics::lint::almost_swapped(
        &span, left.name, right.name,
    ));
    true
}

struct Variable<'a> {
    id: BindingId,
    ty: &'a Type,
    name: &'a str,
}

fn variable(expression: &Expression) -> Option<Variable<'_>> {
    if let Expression::Identifier {
        binding_id: Some(id),
        ty,
        value,
        ..
    } = expression
    {
        Some(Variable {
            id: *id,
            ty,
            name: value.as_str(),
        })
    } else {
        None
    }
}

fn plain_assignment(item: &Expression) -> Option<(&Expression, &Expression)> {
    if let Expression::Assignment {
        target,
        value,
        compound_operator: None,
        ..
    } = item
    {
        Some((target.unwrap_parens(), value.unwrap_parens()))
    } else {
        None
    }
}

fn is_mutable(id: BindingId, ctx: &NodeCtx) -> bool {
    ctx.facts
        .bindings
        .get(&id)
        .is_some_and(|binding| binding.kind.is_mutable())
}
