use super::helpers::{is_side_effect_free, method_call, span_text, time_now_namespace};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

pub fn check_manual_time_until(expression: &Expression, ctx: &NodeCtx) {
    let Some((receiver, [arg], span)) = method_call(expression, "Sub") else {
        return;
    };

    let Some(namespace) = time_now_namespace(arg) else {
        return;
    };

    // No strip_refs: a `Ref<time.Time>` receiver reaches `Sub` via auto-deref, but
    // `time.Until` takes a `Time`, so `time.Until(x)` would not type-check.
    if receiver.get_type().get_qualified_id() != Some("go:time.Time") {
        return;
    }

    if !is_side_effect_free(receiver) {
        return;
    }

    let (Some(namespace_text), Some(receiver_text)) = (
        span_text(ctx.source(), namespace),
        span_text(ctx.source(), receiver),
    ) else {
        return;
    };

    let replacement = format!("{namespace_text}.Until({receiver_text})");

    ctx.sink.push(
        diagnostics::lint::manual_time_until(span, namespace_text, receiver_text).with_fix(
            Fix::new(
                format!("Replace with `{replacement}`"),
                Edit::replacement(*span, replacement.clone()),
            ),
        ),
    );
}
