use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::{is_eager_safe, is_pure_mapper, method_call};

pub fn check_map_unwrap_or(expression: &Expression, ctx: &NodeCtx) {
    let Some((map_call, args, span)) = method_call(expression, "unwrap_or") else {
        return;
    };
    let [default] = args else {
        return;
    };

    // `map_or` evaluates the default before the mapper; the reorder is safe only
    // if both are side-effect-free to evaluate.
    if !is_eager_safe(default) {
        return;
    }

    let map_call = map_call.unwrap_parens();
    let Some((map_receiver, map_args, _)) = method_call(map_call, "map") else {
        return;
    };
    let [mapper] = map_args else {
        return;
    };
    if !is_pure_mapper(mapper) {
        return;
    }

    // `map_or` exists on `Option`/`Result`, not `Partial`.
    let receiver = map_receiver.get_type();
    if !receiver.is_option() && !receiver.is_result() {
        return;
    }

    ctx.sink.push(diagnostics::lint::map_unwrap_or(span));
}
