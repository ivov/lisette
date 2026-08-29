use super::helpers::{is_zero_literal, method_call, span_text};
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_replace_count_zero(expression: &Expression, ctx: &NodeCtx) {
    let Some((namespace, [s, old, new, count], span)) = method_call(expression, "Replace") else {
        return;
    };

    if !is_zero_literal(count.unwrap_parens()) {
        return;
    }

    if namespace.get_type().as_import_namespace() != Some("go:strings") {
        return;
    }

    let (Some(namespace_text), Some(s_text), Some(old_text), Some(new_text)) = (
        span_text(ctx.source(), namespace),
        span_text(ctx.source(), s),
        span_text(ctx.source(), old),
        span_text(ctx.source(), new),
    ) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::replace_count_zero(
        span,
        namespace_text,
        s_text,
        old_text,
        new_text,
    ));
}
