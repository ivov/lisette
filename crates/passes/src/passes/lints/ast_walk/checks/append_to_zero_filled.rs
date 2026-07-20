use super::helpers::{is_bare_identifier, is_zero_literal};
use crate::passes::walk::NodeCtx;
use syntax::ast::{BindingId, Expression, Literal, Pattern, Span};
use syntax::program::{CallKind, NativeTypeKind};

pub fn check_append_to_zero_filled(expression: &Expression, ctx: &NodeCtx) {
    if let Some(receiver) = growing_append_receiver(expression)
        && let Some(make) = zero_filled_make_call(receiver.unwrap_parens())
    {
        ctx.sink.push(diagnostics::lint::append_to_zero_filled(
            &make.span,
            &expression.get_span(),
            make.length,
            &make.element,
        ));
    }

    let Expression::Block { items, .. } = expression else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let Some((binding_id, make)) = zero_filled_make_binding(item, ctx) else {
            continue;
        };
        if let Some(FirstUse::AppendReceiver(append_span)) = items[index + 1..]
            .iter()
            .find_map(|later| first_use(later, binding_id))
        {
            ctx.sink.push(diagnostics::lint::append_to_zero_filled(
                &make.span,
                &append_span,
                make.length,
                &make.element,
            ));
        }
    }
}

struct MakeCall {
    span: Span,
    length: Option<u64>,
    element: String,
}

/// `Slice.make(n)` with `n` not a literal zero, which has no zeros to append after.
fn zero_filled_make_call(expression: &Expression) -> Option<MakeCall> {
    let Expression::Call {
        expression: callee,
        call_kind: Some(CallKind::NativeConstructor(NativeTypeKind::Slice)),
        args,
        span,
        ty,
        ..
    } = expression
    else {
        return None;
    };
    let length_arg = args.first()?;
    if is_zero_literal(length_arg.unwrap_parens()) {
        return None;
    }
    if !is_bare_identifier(callee, "Slice.make") {
        return None;
    }
    let length = match length_arg.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Integer { value, .. },
            ..
        } if *value > 0 => Some(*value),
        _ => None,
    };
    let element = ty
        .get_type_params()
        .and_then(|params| params.first())
        .map(|element| element.to_string())
        .unwrap_or_else(|| "T".to_string());
    Some(MakeCall {
        span: *span,
        length,
        element,
    })
}

/// The binding id comes from the pattern-span-keyed facts, so shadows never match.
fn zero_filled_make_binding(item: &Expression, ctx: &NodeCtx) -> Option<(BindingId, MakeCall)> {
    let Expression::Let { binding, value, .. } = item else {
        return None;
    };
    let Pattern::Identifier { identifier, span } = &binding.pattern else {
        return None;
    };
    let make = zero_filled_make_call(value.unwrap_parens())?;
    let (id, _) = ctx
        .facts
        .bindings
        .iter()
        .find(|(_, fact)| fact.span == *span && fact.name == identifier.as_str())?;
    Some((*id, make))
}

enum FirstUse {
    AppendReceiver(Span),
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
        return Some(FirstUse::AppendReceiver(expression.get_span()));
    }
    if is_binding(expression, binding_id) {
        return Some(FirstUse::Other);
    }
    if let Some((unconditional, branches)) = branch_parts(expression) {
        for part in unconditional {
            if let Some(found) = first_use(part, binding_id) {
                return Some(found);
            }
        }
        return first_use_across_branches(&branches, binding_id);
    }
    for child in expression.children() {
        if let Some(found) = first_use(child, binding_id) {
            return Some(found);
        }
    }
    None
}

/// The condition/scrutinee and the mutually exclusive branch bodies, if any.
fn branch_parts(expression: &Expression) -> Option<(Vec<&Expression>, Vec<&Expression>)> {
    match expression {
        Expression::If {
            condition,
            consequence,
            alternative,
            ..
        } => Some((vec![condition], vec![consequence, alternative])),
        Expression::IfLet {
            scrutinee,
            consequence,
            alternative,
            ..
        } => Some((vec![scrutinee], vec![consequence, alternative])),
        Expression::Match { subject, arms, .. } => Some((
            vec![subject.as_ref()],
            arms.iter().map(|arm| arm.expression.as_ref()).collect(),
        )),
        _ => None,
    }
}

fn first_use_across_branches(branches: &[&Expression], binding_id: BindingId) -> Option<FirstUse> {
    let mut append_span = None;
    for branch in branches {
        match first_use(branch, binding_id) {
            Some(FirstUse::Other) => return Some(FirstUse::Other),
            Some(FirstUse::AppendReceiver(span)) => {
                append_span.get_or_insert(span);
            }
            None => {}
        }
    }
    append_span.map(FirstUse::AppendReceiver)
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
