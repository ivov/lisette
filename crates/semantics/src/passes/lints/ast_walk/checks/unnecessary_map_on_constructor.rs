use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::{
    is_eager_safe, is_identity_lambda, is_pure_mapper, method_call, wrapped_single_arg,
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
        if confirmed && reorder_safe(payload, mapper) {
            ctx.sink
                .push(diagnostics::lint::unnecessary_map_on_constructor(
                    span, variant, "map",
                ));
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
        if receiver.get_type().is_result() && reorder_safe(payload, mapper) {
            ctx.sink
                .push(diagnostics::lint::unnecessary_map_on_constructor(
                    span, "Err", "map_err",
                ));
        }
    }
}

// `Constructor(p).map(f)` becomes `Constructor(f(p))`, swapping evaluation of
// `p` and `f`. Invisible when `f` is a lambda literal, or when both are
// side-effect-free.
fn reorder_safe(payload: &Expression, mapper: &Expression) -> bool {
    if !is_pure_mapper(mapper) {
        return false;
    }
    matches!(mapper.unwrap_parens(), Expression::Lambda { .. }) || is_eager_safe(payload)
}
