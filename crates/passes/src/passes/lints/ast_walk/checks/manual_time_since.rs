use super::helpers::{is_side_effect_free, span_text};
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_manual_time_since(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        span,
        ..
    } = expression
    else {
        return;
    };

    let [arg] = args.as_slice() else {
        return;
    };

    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    if member.as_str() != "Sub" {
        return;
    }

    let Some(namespace) = time_now_namespace(receiver) else {
        return;
    };

    if !is_side_effect_free(arg) {
        return;
    }

    let (Some(namespace_text), Some(arg_text)) =
        (span_text(ctx.source, namespace), span_text(ctx.source, arg))
    else {
        return;
    };

    ctx.sink.push(diagnostics::lint::manual_time_since(
        span,
        namespace_text,
        arg_text,
    ));
}

fn time_now_namespace(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };

    if !args.is_empty() {
        return None;
    }

    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };

    if member.as_str() != "Now" {
        return None;
    }

    if namespace.get_type().as_import_namespace() != Some("go:time") {
        return None;
    }

    Some(namespace)
}
