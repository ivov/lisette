use super::helpers::{expressions_equivalent, is_side_effect_free, is_zero_literal, span_text};
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_redundant_slice_bounds(expression: &Expression, ctx: &NodeCtx) {
    let Expression::IndexedAccess {
        expression: receiver,
        index,
        span,
        ..
    } = expression
    else {
        return;
    };

    let Expression::Range {
        start,
        end,
        inclusive,
        ..
    } = index.unwrap_parens()
    else {
        return;
    };

    if !receiver.get_type().is_slice() {
        return;
    }

    let start_is_zero = start
        .as_deref()
        .is_some_and(|start| is_zero_literal(start.unwrap_parens()));

    // Exclusive only (inclusive `a..=x.length()` is out of bounds, not a
    // redundant `a..`); receiver must be side-effect-free since dropping
    // `.length()` drops one of its evaluations.
    let end_is_length = !inclusive
        && end
            .as_deref()
            .is_some_and(|end| is_length_call_on(end, receiver))
        && is_side_effect_free(receiver);

    // Emit caps a range slice's capacity at its upper bound (`xs[a..b]` lowers to
    // `xs[a:b:b]`), so `xs[..]` differs observably from bare `xs`. Drop only one
    // default bound while a genuine bound survives; never collapse to `xs`/`xs[..]`.
    let drop_start = start_is_zero && end.is_some() && !end_is_length;

    // Open-ending the slice makes emit read the synthesized `len(x)` before the
    // start, reordering it; sound only when the surviving start is side-effect-free.
    let drop_end =
        end_is_length && !start_is_zero && start.as_deref().is_some_and(is_side_effect_free);

    if !(drop_start || drop_end) {
        return;
    }

    let Some(receiver_text) = span_text(ctx.source, receiver) else {
        return;
    };

    let replacement = if drop_start {
        let Some(end_text) = end.as_deref().and_then(|end| span_text(ctx.source, end)) else {
            return;
        };
        let separator = if *inclusive { "..=" } else { ".." };
        format!("{receiver_text}[{separator}{end_text}]")
    } else {
        let Some(start_text) = start
            .as_deref()
            .and_then(|start| span_text(ctx.source, start))
        else {
            return;
        };
        format!("{receiver_text}[{start_text}..]")
    };

    ctx.sink.push(diagnostics::lint::redundant_slice_bounds(
        span,
        &replacement,
    ));
}

fn is_length_call_on(expression: &Expression, slice_receiver: &Expression) -> bool {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return false;
    };
    if !args.is_empty() {
        return false;
    }
    let Expression::DotAccess {
        expression: length_receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return false;
    };
    member == "length" && expressions_equivalent(length_receiver, slice_receiver)
}
