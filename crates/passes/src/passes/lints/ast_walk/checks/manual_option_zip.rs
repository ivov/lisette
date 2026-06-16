use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::{
    is_bare_identifier, is_side_effect_free, mentions_identifier, method_call, unary_lambda,
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

    // `zip` evaluates its argument eagerly and outside the outer closure, so the
    // second option must be side-effect-free and independent of the captured
    // binding.
    if !is_side_effect_free(inner_receiver) || mentions_identifier(inner_receiver, outer_param) {
        return;
    }

    ctx.sink.push(diagnostics::lint::manual_option_zip(span));
}
