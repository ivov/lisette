use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Literal};

use super::helpers::span_text;

pub fn check_redundant_trim_guard(expression: &Expression, ctx: &NodeCtx) {
    let (Expression::Block { items, .. }
    | Expression::TryBlock { items, .. }
    | Expression::RecoverBlock { items, .. }) = expression
    else {
        return;
    };

    // The last item is the block's value, and a bare assignment cannot stand
    // where a value is expected, so only earlier items can take the body. The
    // block's own type does not tell the two apart: a consumed block whose tail
    // is a value-less `if` reads as `()` just like a discarded one.
    let Some((_, statements)) = items.split_last() else {
        return;
    };

    for statement in statements {
        fire_if_redundant(statement, ctx);
    }
}

fn fire_if_redundant(statement: &Expression, ctx: &NodeCtx) {
    let Expression::If {
        condition,
        consequence,
        alternative: None,
        span,
        ..
    } = statement
    else {
        return;
    };

    let Some(guard) = dot_call(condition) else {
        return;
    };
    let (prefix, expected_trim) = match guard.member {
        "HasPrefix" => (true, "TrimPrefix"),
        "HasSuffix" => (false, "TrimSuffix"),
        _ => return,
    };

    let Expression::Block { items, .. } = consequence.as_ref() else {
        return;
    };
    let [
        assignment @ Expression::Assignment {
            target,
            value,
            compound_operator: None,
            ..
        },
    ] = items.as_slice()
    else {
        return;
    };

    let Some(target_id) = target.binding_id() else {
        return;
    };
    if guard.subject.binding_id() != Some(target_id) {
        return;
    }

    let Some(trim) = dot_call(value) else {
        return;
    };
    if trim.member != expected_trim || trim.subject.binding_id() != Some(target_id) {
        return;
    }
    if !same_affix(guard.affix, trim.affix) {
        return;
    }

    if !is_strings_namespace(guard.namespace) || !is_strings_namespace(trim.namespace) {
        return;
    }

    let (Some(namespace), Some(replacement), Some(guarded)) = (
        span_text(ctx.source, trim.namespace),
        span_text(ctx.source, assignment),
        span_text(ctx.source, statement),
    ) else {
        return;
    };

    // Promoting the body discards, relocates, or extends the reach of any
    // comment the `if` span holds. A comment can end up inside the assignment
    // span without its newline, where it would swallow whatever followed the
    // `if`, so drop the diagnostic instead of only the fix. A `//` inside a
    // string literal silences the lint too rather than being lexed apart.
    if guarded.contains("//") {
        return;
    }

    ctx.sink.push(
        diagnostics::lint::redundant_trim_guard(span, namespace, prefix, replacement).with_fix(
            Fix::new(
                format!("Replace with `{replacement}`"),
                Edit::replacement(*span, replacement.to_string()),
            ),
        ),
    );
}

struct DotCall<'a> {
    namespace: &'a Expression,
    member: &'a str,
    subject: &'a Expression,
    affix: &'a Expression,
}

fn dot_call(expression: &Expression) -> Option<DotCall<'_>> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    let [subject, affix] = args.as_slice() else {
        return None;
    };
    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    Some(DotCall {
        namespace,
        member: member.as_str(),
        subject,
        affix,
    })
}

fn is_strings_namespace(namespace: &Expression) -> bool {
    namespace.get_type().as_import_namespace() == Some("go:strings")
}

fn same_affix(a: &Expression, b: &Expression) -> bool {
    if let Some(id) = a.binding_id() {
        return b.binding_id() == Some(id);
    }
    matches!(
        (a.unwrap_parens(), b.unwrap_parens()),
        (
            Expression::Literal {
                literal: left @ Literal::String { .. },
                ..
            },
            Expression::Literal {
                literal: right @ Literal::String { .. },
                ..
            },
        ) if left == right
    )
}
