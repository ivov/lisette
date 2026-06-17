use super::helpers::{expression_is_pure, is_zero_literal, span_text};
use crate::passes::walk::NodeCtx;
use semantics::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::{CallKind, NativeTypeKind};

pub fn check_manual_find(expression: &Expression, ctx: &NodeCtx) {
    let Some((span, filter_call, index)) = native_slice_method(expression, "get") else {
        return;
    };

    if !is_zero_literal(index.unwrap_parens()) {
        return;
    }

    let Some((_, receiver, predicate)) = native_slice_method(filter_call.unwrap_parens(), "filter")
    else {
        return;
    };

    if !predicate_is_pure(predicate, ctx.store) {
        return;
    }

    let (Some(receiver_text), Some(predicate_text)) = (
        span_text(ctx.source, receiver),
        span_text(ctx.source, predicate),
    ) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::manual_find(
        span,
        receiver_text,
        predicate_text,
    ));
}

fn native_slice_method<'a>(
    expression: &'a Expression,
    name: &str,
) -> Option<(&'a Span, &'a Expression, &'a Expression)> {
    let Expression::Call {
        expression: callee,
        args,
        call_kind,
        span,
        ..
    } = expression
    else {
        return None;
    };

    if !matches!(
        call_kind,
        Some(CallKind::NativeMethod(NativeTypeKind::Slice))
    ) {
        return None;
    }

    let [arg] = args.as_slice() else {
        return None;
    };

    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };

    (member.as_str() == name).then_some((span, receiver.as_ref(), arg))
}

// `filter` evaluates the predicate on every element but `find` short-circuits, so
// the rewrite is sound only for a pure, non-panicking inline closure body.
fn predicate_is_pure(predicate: &Expression, store: &Store) -> bool {
    let Expression::Lambda { body, .. } = predicate.unwrap_parens() else {
        return false;
    };
    expression_is_pure(body, store)
}
