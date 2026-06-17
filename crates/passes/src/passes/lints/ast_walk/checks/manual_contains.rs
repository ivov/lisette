use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::{
    expression_is_pure, is_bare_identifier, mentions_identifier, method_call, unary_lambda,
};

pub fn check_manual_contains(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, args, span)) = method_call(expression, "any") else {
        return;
    };
    let [closure] = args else {
        return;
    };
    let Some((param, body)) = unary_lambda(closure) else {
        return;
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
        span: comparison_span,
        ..
    } = body
    else {
        return;
    };

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    let (element, value) = if is_bare_identifier(left, param) {
        (left, right)
    } else if is_bare_identifier(right, param) {
        (right, left)
    } else {
        return;
    };

    // `contains` evaluates the value once; `any` evaluates it per element, so it
    // must be element-independent and safe to hoist.
    if mentions_identifier(value, param) || !expression_is_pure(value, ctx.store) {
        return;
    }

    if !receiver.get_type().is_slice() {
        return;
    }

    // A checker-rejected `==` is no membership test and could not become `contains`.
    if ctx.facts.type_error_spans.contains(comparison_span) {
        return;
    }

    // An accepted `==` still allows mismatches `contains` rejects, e.g. an `int8`
    // value against a `Slice<int>`.
    if element.get_type() != value.get_type() {
        return;
    }

    ctx.sink.push(diagnostics::lint::manual_contains(span));
}
