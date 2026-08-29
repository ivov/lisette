use super::helpers::{is_side_effect_free, method_call, span_text, time_now_namespace};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

pub fn check_manual_time_since(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, [arg], span)) = method_call(expression, "Sub") else {
        return;
    };

    let Some(namespace) = time_now_namespace(receiver) else {
        return;
    };

    if !is_side_effect_free(arg) {
        return;
    }

    let (Some(namespace_text), Some(arg_text)) = (
        span_text(ctx.source(), namespace),
        span_text(ctx.source(), arg),
    ) else {
        return;
    };

    let replacement = format!("{namespace_text}.Since({arg_text})");

    ctx.sink.push(
        diagnostics::lint::manual_time_since(span, namespace_text, arg_text).with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement.clone()),
        )),
    );
}
