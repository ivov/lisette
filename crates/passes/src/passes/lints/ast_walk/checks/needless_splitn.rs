use super::helpers::{signed_integer_literal, span_text};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

pub fn check_needless_splitn(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        span,
        ..
    } = expression
    else {
        return;
    };

    let [s, sep, count] = args.as_slice() else {
        return;
    };

    // A negative count means "no limit", the exact behaviour of `Split`. A zero
    // count returns nil and a positive count caps the result, so neither matches.
    if signed_integer_literal(count.unwrap_parens()).is_none_or(|value| value >= 0) {
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

    let target = match member.as_str() {
        "SplitN" => "Split",
        "SplitAfterN" => "SplitAfter",
        _ => return,
    };

    if namespace.get_type().as_import_namespace() != Some("go:strings") {
        return;
    }

    let (Some(namespace_text), Some(s_text), Some(sep_text), Some(count_text)) = (
        span_text(ctx.source, namespace),
        span_text(ctx.source, s),
        span_text(ctx.source, sep),
        span_text(ctx.source, count),
    ) else {
        return;
    };

    let replacement = format!("{namespace_text}.{target}({s_text}, {sep_text})");
    ctx.sink.push(
        diagnostics::lint::needless_splitn(
            span,
            member.as_str(),
            target,
            namespace_text,
            s_text,
            sep_text,
            count_text,
        )
        .with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement.clone()),
        )),
    );
}
