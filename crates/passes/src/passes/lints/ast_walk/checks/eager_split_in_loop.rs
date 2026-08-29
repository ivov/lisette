use super::helpers::{replacement_drops_comment, span_text};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

const SEQ_FORMS: [(&str, &str, usize); 3] = [
    ("Split", "SplitSeq", 2),
    ("SplitAfter", "SplitAfterSeq", 2),
    ("Fields", "FieldsSeq", 1),
];

pub fn check_eager_split_in_loop(expression: &Expression, ctx: &NodeCtx) {
    let Expression::For { iterable, .. } = expression else {
        return;
    };

    let Expression::Call {
        expression: callee,
        args,
        spread,
        span,
        ..
    } = iterable.unwrap_parens()
    else {
        return;
    };

    let Expression::DotAccess {
        expression: namespace,
        member,
        span: callee_span,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    let Some(&(_, target, argument_count)) =
        SEQ_FORMS.iter().find(|form| form.0 == member.as_str())
    else {
        return;
    };

    if namespace.get_type().as_import_namespace() != Some("go:strings") {
        return;
    }

    if args.len() != argument_count || spread.is_some() {
        return;
    }

    let Some(namespace_text) = span_text(ctx.source(), namespace) else {
        return;
    };
    let mut arg_texts = Vec::with_capacity(args.len());
    for arg in args {
        let Some(text) = span_text(ctx.source(), arg) else {
            return;
        };
        arg_texts.push(text);
    }
    let arg_list = arg_texts.join(", ");

    let renamed = format!("{namespace_text}.{target}");
    let original = format!("{namespace_text}.{member}({arg_list})");
    let replacement = format!("{renamed}({arg_list})");

    let mut diagnostic = diagnostics::lint::eager_split_in_loop(span, &original, &replacement);
    if !replacement_drops_comment(ctx.source(), *callee_span, &renamed) {
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{renamed}`"),
            Edit::replacement(*callee_span, renamed),
        ));
    }
    ctx.sink.push(diagnostic);
}
