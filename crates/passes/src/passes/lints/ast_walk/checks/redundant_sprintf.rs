use super::helpers::{method_call, span_text};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Literal};

pub fn check_redundant_sprintf(expression: &Expression, ctx: &NodeCtx) {
    let Some((namespace, [format, value], span)) = method_call(expression, "Sprintf") else {
        return;
    };

    let Expression::Literal {
        literal: Literal::String {
            value: format_value,
            ..
        },
        ..
    } = format.unwrap_parens()
    else {
        return;
    };

    if format_value != "%s" {
        return;
    }

    if namespace.get_type().as_import_namespace() != Some("go:fmt") {
        return;
    }

    if !value.get_type().is_string() {
        return;
    }

    let (Some(namespace_text), Some(value_text)) = (
        span_text(ctx.source(), namespace),
        span_text(ctx.source(), value),
    ) else {
        return;
    };

    ctx.sink.push(
        diagnostics::lint::redundant_sprintf(span, namespace_text, value_text).with_fix(Fix::new(
            format!("Replace with `{value_text}`"),
            Edit::replacement(*span, value_text),
        )),
    );
}
