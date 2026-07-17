use super::helpers::{is_bare_identifier, is_zero_literal};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BindingId, Expression, Pattern, Span};
use syntax::program::{CallKind, NativeTypeKind};

pub fn check_append_to_zero_filled(expression: &Expression, ctx: &NodeCtx) {
    if let Some(receiver) = growing_append_receiver(expression)
        && let Some(make_span) = zero_filled_make_call(receiver.unwrap_parens())
    {
        ctx.sink
            .push(diagnostics::lint::append_to_zero_filled(&make_span));
    }

    let Expression::Block { items, .. } = expression else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let Some((binding_id, make_span)) = zero_filled_make_binding(item, ctx) else {
            continue;
        };
        if let Some(FirstUse::AppendReceiver) = items[index + 1..]
            .iter()
            .find_map(|later| first_use(later, binding_id))
        {
            ctx.sink
                .push(diagnostics::lint::append_to_zero_filled(&make_span));
        }
    }
}

/// `Slice.make(n)` with `n` not a literal zero, which has no zeros to append after.
fn zero_filled_make_call(expression: &Expression) -> Option<Span> {
    let Expression::Call {
        expression: callee,
        call_kind: Some(CallKind::NativeConstructor(NativeTypeKind::Slice)),
        args,
        span,
        ..
    } = expression
    else {
        return None;
    };
    let length = args.first()?;
    if is_zero_literal(length.unwrap_parens()) {
        return None;
    }
    is_bare_identifier(callee, "Slice.make").then_some(*span)
}

/// The binding id comes from the pattern-span-keyed facts, so shadows never match.
fn zero_filled_make_binding(item: &Expression, ctx: &NodeCtx) -> Option<(BindingId, Span)> {
    let Expression::Let { binding, value, .. } = item else {
        return None;
    };
    let Pattern::Identifier { identifier, span } = &binding.pattern else {
        return None;
    };
    let make_span = zero_filled_make_call(value.unwrap_parens())?;
    let (id, _) = ctx
        .facts
        .bindings
        .iter()
        .find(|(_, fact)| fact.span == *span && fact.name == identifier.as_str())?;
    Some((*id, make_span))
}

enum FirstUse {
    AppendReceiver,
    Other,
}

/// A reassignment target is not a use, so `x = x.append(1)` classifies by its
/// right side, and a right side not involving `x` replaces the tracked value.
fn first_use(expression: &Expression, binding_id: BindingId) -> Option<FirstUse> {
    if let Expression::Assignment { target, value, .. } = expression
        && is_binding(target, binding_id)
    {
        return first_use(value, binding_id).or(Some(FirstUse::Other));
    }
    if let Some(receiver) = growing_append_receiver(expression)
        && is_binding(receiver, binding_id)
    {
        return Some(FirstUse::AppendReceiver);
    }
    if is_binding(expression, binding_id) {
        return Some(FirstUse::Other);
    }
    for child in expression.children() {
        if let Some(found) = first_use(child, binding_id) {
            return Some(found);
        }
    }
    None
}

/// The receiver of a `receiver.append(...)` or `Slice.append(receiver, ...)`
/// call that appends at least one element.
fn growing_append_receiver(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        call_kind,
        ..
    } = expression
    else {
        return None;
    };
    match call_kind {
        Some(CallKind::NativeMethod(NativeTypeKind::Slice)) => {
            let Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } = callee.unwrap_parens()
            else {
                return None;
            };
            (member == "append" && (!args.is_empty() || spread.is_some()))
                .then(|| receiver.as_ref())
        }
        Some(CallKind::NativeMethodIdentifier(NativeTypeKind::Slice)) => {
            if !is_bare_identifier(callee, "Slice.append") {
                return None;
            }
            (args.len() > 1 || spread.is_some())
                .then(|| args.first())
                .flatten()
        }
        _ => None,
    }
}

fn is_binding(expression: &Expression, binding_id: BindingId) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Identifier { binding_id: Some(id), .. } if *id == binding_id
    )
}
