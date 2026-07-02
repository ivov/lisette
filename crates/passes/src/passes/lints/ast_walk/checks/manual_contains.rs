use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BinaryOperator, Expression};

use super::helpers::{
    as_tight_operand, expression_is_pure, is_bare_identifier, lambda_is_annotated,
    mentions_identifier, method_call, reads_as_method_call, span_text, unary_lambda,
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

    let mut diagnostic = diagnostics::lint::manual_contains(span);
    if !lambda_is_annotated(closure)
        && reads_as_method_call(receiver, args)
        && let (Some(receiver_text), Some(value_text)) = (
            span_text(ctx.source, receiver),
            span_text(ctx.source, value),
        )
    {
        let replacement = format!(
            "{}.contains({value_text})",
            as_tight_operand(receiver_text, receiver)
        );
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}
