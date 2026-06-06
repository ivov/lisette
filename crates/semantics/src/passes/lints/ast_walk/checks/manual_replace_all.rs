use super::helpers::{is_one_literal, span_text};
use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, UnaryOperator};

pub fn check_manual_replace_all(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        span,
        ..
    } = expression
    else {
        return;
    };

    let [s, old, new, count] = args.as_slice() else {
        return;
    };

    if !is_negative_one(count.unwrap_parens()) {
        return;
    }

    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    if member.as_str() != "Replace" {
        return;
    }

    if namespace.get_type().as_import_namespace() != Some("go:strings") {
        return;
    }

    let (Some(namespace_text), Some(s_text), Some(old_text), Some(new_text)) = (
        span_text(ctx.source, namespace),
        span_text(ctx.source, s),
        span_text(ctx.source, old),
        span_text(ctx.source, new),
    ) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::manual_replace_all(
        span,
        namespace_text,
        s_text,
        old_text,
        new_text,
    ));
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
