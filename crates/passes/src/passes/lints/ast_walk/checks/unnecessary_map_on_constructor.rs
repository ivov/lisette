use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use semantics::store::Store;
use syntax::ast::{Expression, Span};

use super::helpers::{
    as_tight_operand, expression_is_pure, is_identity_lambda, method_call, reads_as_method_call,
    span_text, wrapped_single_arg,
};

pub fn check_unnecessary_map_on_constructor(expression: &Expression, ctx: &NodeCtx) {
    if let Some((receiver, args, span)) = method_call(expression, "map") {
        let [mapper] = args else {
            return;
        };
        // `.map(|x| x)` is owned by `map_identity`.
        if is_identity_lambda(mapper) {
            return;
        }
        let receiver = receiver.unwrap_parens();
        let (variant, payload) = if let Some(payload) = wrapped_single_arg(receiver, "Some") {
            ("Some", payload)
        } else if let Some(payload) = wrapped_single_arg(receiver, "Ok") {
            ("Ok", payload)
        } else {
            return;
        };
        // Confirm the prelude constructor, not a same-named user function.
        let container = receiver.get_type();
        let confirmed = match variant {
            "Some" => container.is_option(),
            _ => container.is_result(),
        };
        if confirmed && reorder_safe(payload, mapper, ctx.store) {
            push_map_on_constructor(ctx, span, receiver, args, variant, "map", payload, mapper);
        }
        return;
    }

    if let Some((receiver, args, span)) = method_call(expression, "map_err") {
        let [mapper] = args else {
            return;
        };
        let receiver = receiver.unwrap_parens();
        let Some(payload) = wrapped_single_arg(receiver, "Err") else {
            return;
        };
        if receiver.get_type().is_result() && reorder_safe(payload, mapper, ctx.store) {
            push_map_on_constructor(ctx, span, receiver, args, "Err", "map_err", payload, mapper);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_map_on_constructor(
    ctx: &NodeCtx,
    span: &Span,
    receiver: &Expression,
    args: &[Expression],
    variant: &str,
    method: &str,
    payload: &Expression,
    mapper: &Expression,
) {
    let mut diagnostic = diagnostics::lint::unnecessary_map_on_constructor(span, variant, method);
    // A lambda mapper needs beta-reduction to inline, so it reports without a fix.
    if !matches!(mapper.unwrap_parens(), Expression::Lambda { .. })
        && reads_as_method_call(receiver, args)
        && let (Some(mapper_text), Some(payload_text)) = (
            span_text(ctx.source, mapper),
            span_text(ctx.source, payload),
        )
    {
        let replacement = format!(
            "{variant}({}({payload_text}))",
            as_tight_operand(mapper_text, mapper)
        );
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}

// `Constructor(p).map(f)` to `Constructor(f(p))` swaps evaluation of `p` and `f`.
// A lambda defers its body, otherwise both must be non-panicking for the swap to
// be invisible.
fn reorder_safe(payload: &Expression, mapper: &Expression, store: &Store) -> bool {
    if matches!(mapper.unwrap_parens(), Expression::Lambda { .. }) {
        return true;
    }
    expression_is_pure(payload, store) && expression_is_pure(mapper, store)
}
