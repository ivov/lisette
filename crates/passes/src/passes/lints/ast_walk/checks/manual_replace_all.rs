use super::helpers::{is_one_literal, method_call, span_text};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, UnaryOperator};

pub fn check_manual_replace_all(expression: &Expression, ctx: &NodeCtx) {
    let Some((namespace, [s, old, new, count], span)) = method_call(expression, "Replace") else {
        return;
    };

    if !is_negative_one(count.unwrap_parens()) {
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

    let replacement = format!("{namespace_text}.ReplaceAll({s_text}, {old_text}, {new_text})");

    ctx.sink.push(
        diagnostics::lint::manual_replace_all(span, namespace_text, s_text, old_text, new_text)
            .with_fix(Fix::new(
                format!("Replace with `{replacement}`"),
                Edit::replacement(*span, replacement.clone()),
            )),
    );
}

fn is_negative_one(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression: inner,
            ..
        } if is_one_literal(inner.unwrap_parens())
    )
}
