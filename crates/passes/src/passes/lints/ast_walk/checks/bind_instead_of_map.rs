use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::{has_escaping_control_flow, method_call, unary_lambda, wrapped_single_arg};

pub fn check_bind_instead_of_map(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, args, span)) = method_call(expression, "and_then") else {
        return;
    };
    let [closure] = args else {
        return;
    };
    let Some((_, body)) = unary_lambda(closure) else {
        return;
    };

    let (wrapper, wrapped) = if let Some(arg) = wrapped_single_arg(body, "Some") {
        ("Some", arg)
    } else if let Some(arg) = wrapped_single_arg(body, "Ok") {
        ("Ok", arg)
    } else {
        return;
    };

    // A `?` here is valid in the `and_then` closure but not in a `map` one.
    if has_escaping_control_flow(wrapped) {
        return;
    }

    let container = receiver.get_type();
    let confirmed = match wrapper {
        "Some" => container.is_option(),
        _ => container.is_result(),
    };
    if !confirmed {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::bind_instead_of_map(span, wrapper));
}
