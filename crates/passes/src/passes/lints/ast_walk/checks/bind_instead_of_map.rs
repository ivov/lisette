use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::{
    as_tight_operand, has_escaping_control_flow, lambda_is_annotated, method_call,
    reads_as_method_call, span_text, unary_lambda, wrapped_single_arg,
};

pub fn check_bind_instead_of_map(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, args, span)) = method_call(expression, "and_then") else {
        return;
    };
    let [closure] = args else {
        return;
    };
    let Some((param, body)) = unary_lambda(closure) else {
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

    let mut diagnostic = diagnostics::lint::bind_instead_of_map(span, wrapper);
    if !lambda_is_annotated(closure)
        && reads_as_method_call(receiver, args)
        && let (Some(receiver_text), Some(wrapped_text)) = (
            span_text(ctx.source, receiver),
            span_text(ctx.source, wrapped),
        )
    {
        let replacement = format!(
            "{}.map(|{param}| {wrapped_text})",
            as_tight_operand(receiver_text, receiver)
        );
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}
