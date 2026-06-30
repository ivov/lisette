use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::{is_identity_lambda, method_call, span_text};

pub fn check_map_identity(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, args, span)) = method_call(expression, "map") else {
        return;
    };
    let [closure] = args else {
        return;
    };
    if !is_identity_lambda(closure) {
        return;
    }

    // Not slices: dropping `.map(|x| x)` there would drop the copy it makes.
    let container = receiver.get_type();
    if !container.is_option() && !container.is_result() {
        return;
    }

    // `.map(|x| x)` can upcast (`Option<Text>` to `Option<Printable>`); only a
    // type-preserving map is removable.
    if container != expression.get_type() {
        return;
    }

    let mut diagnostic = diagnostics::lint::map_identity(span);
    if let Some(text) = span_text(ctx.source, receiver) {
        diagnostic = diagnostic.with_fix(Fix::new(
            "Remove identity map",
            Edit::replacement(*span, text),
        ));
    }
    ctx.sink.push(diagnostic);
}
