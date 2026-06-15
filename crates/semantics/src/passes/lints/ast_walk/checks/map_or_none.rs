use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;
use syntax::types::Type;

use super::helpers::{is_bare_identifier, method_call, unary_lambda, wrapped_single_arg};

pub fn check_map_or_none(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, args, span)) = method_call(expression, "map_or") else {
        return;
    };
    let [default, func] = args else {
        return;
    };

    if !is_bare_identifier(default, "None") || !default.get_type().is_option() {
        return;
    }

    let container = receiver.get_type();
    if container.is_option() {
        // Require `f` to return `Option` so the rewrite holds on rejected calls.
        if func
            .get_type()
            .get_function_ret()
            .is_some_and(Type::is_option)
        {
            ctx.sink
                .push(diagnostics::lint::map_or_none(span, "and_then"));
        }
    } else if container.is_result() && returns_some(func) {
        // `.ok()` yields `Option<T>` from the `Ok` type, dropping any upcast the
        // closure applied; only suggest it when the result is that `Option<T>`.
        let result_ty = expression.get_type();
        let ok_type = container
            .get_type_params()
            .and_then(|params| params.first());
        let result_inner = result_ty
            .get_type_params()
            .and_then(|params| params.first());
        if ok_type.is_some() && ok_type == result_inner {
            ctx.sink.push(diagnostics::lint::map_or_none(span, "ok"));
        }
    }
}

fn returns_some(func: &Expression) -> bool {
    if is_bare_identifier(func, "Some") {
        return true;
    }
    let Some((param, body)) = unary_lambda(func) else {
        return false;
    };
    wrapped_single_arg(body, "Some").is_some_and(|arg| is_bare_identifier(arg, param))
}
