use super::helpers::span_text;
use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::types::SimpleKind;

pub fn check_redundant_fstring_conversion(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Literal {
        literal: Literal::FormatString(parts),
        ..
    } = expression
    else {
        return;
    };

    // Solo f-strings are owned by `expression_only_fstring`.
    if parts.len() < 2 {
        return;
    }

    for part in parts {
        let FormatStringPart::Expression(value) = part else {
            continue;
        };
        let call = value.unwrap_parens();
        let Some((method, arg)) = redundant_conversion(call) else {
            continue;
        };
        let Some(arg_text) = span_text(ctx.source, arg) else {
            continue;
        };
        ctx.sink
            .push(diagnostics::lint::redundant_fstring_conversion(
                &call.get_span(),
                method,
                arg_text,
            ));
    }
}

// Each arm gates the arg type so `f"{arg}"` formats identically and is valid,
// which also keeps the lint off checker-rejected calls.
fn redundant_conversion(call: &Expression) -> Option<(&'static str, &Expression)> {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        ..
    } = call
    else {
        return None;
    };

    if spread.is_some() {
        return None;
    }

    let [arg] = args.as_slice() else {
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

    let namespace_ty = namespace.get_type();
    let namespace = namespace_ty.as_import_namespace();
    let arg_ty = arg.get_type();

    match (namespace, member.as_str()) {
        (Some("go:strconv"), "Itoa") if arg_ty.is_simple(SimpleKind::Int) => Some(("Itoa", arg)),
        (Some("go:strconv"), "FormatBool") if arg_ty.is_boolean() => Some(("FormatBool", arg)),
        // `%v` matches the f-string verb for every scalar but `rune`, where it
        // prints the codepoint while the f-string uses `%c`.
        (Some("go:fmt"), "Sprint")
            if arg_ty
                .as_simple()
                .is_some_and(|kind| kind != SimpleKind::Rune) =>
        {
            Some(("Sprint", arg))
        }
        _ => None,
    }
}
