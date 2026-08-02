use super::helpers::{
    is_bare_identifier, mentions_identifier, method_call, replacement_drops_comment,
};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BindingId, Expression, IdentifierResolution, Literal, Pattern, Span};
use syntax::program::{CallKind, NativeTypeKind};
use syntax::types::Type;

pub fn check_manual_extend(expression: &Expression, ctx: &NodeCtx) {
    let (Expression::Block { items, .. }
    | Expression::TryBlock { items, .. }
    | Expression::RecoverBlock { items, .. }) = expression
    else {
        return;
    };
    // Resolving a declaration scans every binding in the program.
    if !items
        .iter()
        .any(|item| matches!(item, Expression::For { .. }))
    {
        return;
    }

    for (index, item) in items.iter().enumerate() {
        let Some(accumulator) = fresh_slice_declaration(item, ctx) else {
            continue;
        };
        for later in &items[index + 1..] {
            if let Some((span, iterable)) = extending_loop(later, &accumulator, ctx) {
                report(span, accumulator.name, iterable, ctx);
                break;
            }
            // A mention could rebind the accumulator to a slice that shares
            // storage with the iterated one.
            if mentions_identifier(later, accumulator.name) {
                break;
            }
        }
    }
}

struct Accumulator<'a> {
    id: BindingId,
    name: &'a str,
}

/// A `let mut` slice whose storage is newly allocated and full, so the first
/// append reallocates rather than writing into another slice's storage.
fn fresh_slice_declaration<'a>(item: &'a Expression, ctx: &NodeCtx) -> Option<Accumulator<'a>> {
    let Expression::Let { binding, value, .. } = item else {
        return None;
    };
    if !binding.is_mutable() {
        return None;
    }
    let Pattern::Identifier { identifier, span } = &binding.pattern else {
        return None;
    };
    let is_fresh = match value.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Slice(_),
            ty,
            ..
        } => ty.is_slice(),
        Expression::Call {
            expression: callee,
            call_kind: CallKind::NativeConstructor(NativeTypeKind::Slice),
            args,
            ..
        } => args.is_empty() && is_bare_identifier(callee, "Slice.new"),
        _ => false,
    };
    if !is_fresh {
        return None;
    }
    Some(Accumulator {
        id: ctx.facts.binding_id_at(*span)?,
        name: identifier.as_str(),
    })
}

/// Matches `for element in iterable { accumulator = accumulator.append(element) }`.
fn extending_loop<'a>(
    item: &'a Expression,
    accumulator: &Accumulator,
    ctx: &NodeCtx,
) -> Option<(Span, &'a str)> {
    let Expression::For {
        binding,
        iterable,
        body,
        span,
    } = item
    else {
        return None;
    };

    let iterable = local_binding(iterable)?;
    if iterable.id == accumulator.id || !iterable.ty.is_slice() || iterable.ty.contains_error() {
        return None;
    }

    let Expression::Block { items, .. } = body.as_ref() else {
        return None;
    };
    let [
        Expression::Assignment {
            target,
            value,
            compound_operator: None,
            ..
        },
    ] = items.as_slice()
    else {
        return None;
    };
    if target.binding_id() != Some(accumulator.id) {
        return None;
    }

    let value = value.unwrap_parens();
    let Expression::Call {
        spread: None,
        call_kind: CallKind::NativeMethod(NativeTypeKind::Slice),
        ..
    } = value
    else {
        return None;
    };
    let Some((receiver, [element], _)) = method_call(value, "append") else {
        return None;
    };
    let receiver = local_binding(receiver)?;
    if receiver.id != accumulator.id {
        return None;
    }

    let Pattern::Identifier {
        span: element_span, ..
    } = &binding.pattern
    else {
        return None;
    };
    if element.binding_id() != Some(ctx.facts.binding_id_at(*element_span)?) {
        return None;
    }

    // Per-element append widens each element to the accumulator's element type,
    // the spread form does not.
    if receiver.ty != iterable.ty {
        return None;
    }

    Some((*span, iterable.name))
}

struct Local<'a> {
    id: BindingId,
    name: &'a str,
    ty: &'a Type,
}

fn local_binding(expression: &Expression) -> Option<Local<'_>> {
    let Expression::Identifier {
        value,
        resolution: IdentifierResolution::Binding(id),
        ty,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    Some(Local {
        id: *id,
        name: value.as_str(),
        ty,
    })
}

fn report(span: Span, accumulator: &str, iterable: &str, ctx: &NodeCtx) {
    let replacement = format!("{accumulator} = {accumulator}.append({iterable}...)");
    let mut diagnostic = diagnostics::lint::manual_extend(&span, &replacement);
    if !replacement_drops_comment(ctx.source(), span, &replacement) {
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}
