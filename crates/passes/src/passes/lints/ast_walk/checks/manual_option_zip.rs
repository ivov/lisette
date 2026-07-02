use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;
use syntax::types::Type;

use super::helpers::{
    as_tight_operand, expression_is_pure, is_bare_identifier, is_side_effect_free,
    lambda_is_annotated, mentions_identifier, method_call, reads_as_method_call, span_text,
    unary_lambda,
};

pub fn check_manual_option_zip(expression: &Expression, ctx: &NodeCtx) {
    let Some((outer_receiver, outer_args, span)) = method_call(expression, "and_then") else {
        return;
    };
    let [outer_closure] = outer_args else {
        return;
    };
    if !outer_receiver.get_type().is_option() {
        return;
    }
    let Some((outer_param, outer_body)) = unary_lambda(outer_closure) else {
        return;
    };

    let Some((inner_receiver, inner_args, _)) = method_call(outer_body.unwrap_parens(), "map")
    else {
        return;
    };
    let [inner_closure] = inner_args else {
        return;
    };
    if !inner_receiver.get_type().is_option() {
        return;
    }
    let Some((inner_param, inner_body)) = unary_lambda(inner_closure) else {
        return;
    };

    // `(a, a)` would reference the inner binding twice, not the captured outer
    // one, so the names must differ for this to be a zip.
    if outer_param == inner_param {
        return;
    }
    let Expression::Tuple { elements, .. } = inner_body.unwrap_parens() else {
        return;
    };
    let [first, second] = elements.as_slice() else {
        return;
    };
    if !is_bare_identifier(first, outer_param) || !is_bare_identifier(second, inner_param) {
        return;
    }

    // `zip` evaluates the second option eagerly and outside the outer closure, so
    // it must not depend on the binding or panic where the lazy `and_then` would not.
    if !is_side_effect_free(inner_receiver)
        || mentions_identifier(inner_receiver, outer_param)
        || !expression_is_pure(inner_receiver, ctx.store)
    {
        return;
    }

    // `and_then`/`map` can adapt the tuple elements against the result type, which
    // `zip` (fixing them from the receivers) would not, so require a matching type.
    let outer_ty = outer_receiver.get_type();
    let inner_ty = inner_receiver.get_type();
    let result_ty = expression.get_type();
    let (Some(first_ty), Some(second_ty), Some(result_inner)) = (
        option_inner(&outer_ty),
        option_inner(&inner_ty),
        option_inner(&result_ty),
    ) else {
        return;
    };
    if *result_inner != Type::Tuple(vec![first_ty.clone(), second_ty.clone()]) {
        return;
    }

    let mut diagnostic = diagnostics::lint::manual_option_zip(span);
    if !lambda_is_annotated(outer_closure)
        && !lambda_is_annotated(inner_closure)
        && reads_as_method_call(outer_receiver, outer_args)
        && let (Some(receiver_text), Some(other_text)) = (
            span_text(ctx.source, outer_receiver),
            span_text(ctx.source, inner_receiver),
        )
    {
        let replacement = format!(
            "{}.zip({other_text})",
            as_tight_operand(receiver_text, outer_receiver)
        );
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}

fn option_inner(ty: &Type) -> Option<&Type> {
    if !ty.is_option() {
        return None;
    }
    match ty {
        Type::Nominal { params, .. } => params.first(),
        _ => None,
    }
}
