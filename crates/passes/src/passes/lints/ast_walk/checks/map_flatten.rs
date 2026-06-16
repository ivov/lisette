use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::method_call;

pub fn check_map_flatten(expression: &Expression, ctx: &NodeCtx) {
    let Some((map_call, args, span)) = method_call(expression, "flatten") else {
        return;
    };
    if !args.is_empty() {
        return;
    }

    let map_call = map_call.unwrap_parens();
    let Some((map_receiver, map_args, _)) = method_call(map_call, "map") else {
        return;
    };
    if map_args.len() != 1 {
        return;
    }

    // `.and_then` is suggested on the `.map` receiver, which must be an `Option`.
    if !map_receiver.get_type().is_option() {
        return;
    }

    ctx.sink.push(diagnostics::lint::map_flatten(span));
}
