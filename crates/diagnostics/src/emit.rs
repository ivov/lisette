use crate::LisetteDiagnostic;
use crate::pattern;
use syntax::ast::Span;

pub fn go_name_collision(
    go_name: &str,
    first: &Span,
    second: &Span,
    rest: &[Span],
    detail: Option<&str>,
) -> LisetteDiagnostic {
    let mut diagnostic = LisetteDiagnostic::error(format!("Go name collision on `{}`", go_name));
    for (index, span) in [first, second].into_iter().chain(rest).enumerate() {
        let label = format!("becomes `{}` in Go", go_name);
        diagnostic = if index == 0 {
            diagnostic.with_span_primary_label(span, label)
        } else {
            diagnostic.with_span_label(span, label)
        };
    }
    let mut help = format!(
        "Rename all but one. These declarations all become `{}` in generated Go, \
         but Go requires package-level names to be distinct.",
        go_name
    );
    if let Some(detail) = detail {
        help.push(' ');
        help.push_str(detail);
    }
    diagnostic
        .with_emit_code("go_name_collision")
        .with_help(help)
}

pub fn reserved_go_prefix(name: &str, prefix: &str, span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reserved Go name prefix")
        .with_emit_code("reserved_go_prefix")
        .with_span_primary_label(span, "uses a reserved prefix")
        .with_help(format!(
            "Rename the declaration. `{}` starts with `{}`, which is reserved for \
             compiler-generated code.",
            name, prefix
        ))
}

pub fn unnamable_go_type(go_type: &str, span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Private Go type in a typed position")
        .with_emit_code("unnamable_go_type")
        .with_span_primary_label(
            span,
            format!("the Go code for this needs the type `{go_type}`"),
        )
        .with_help(
            "Only its own Go package can name this type. Bind the value with `let`, \
             pass it to a call, or call its methods, so that Go infers the type",
        )
}

pub fn type_without_go_form(what: &str, span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Internal compiler error")
        .with_emit_code("type_without_go_form")
        .with_span_primary_label(
            span,
            format!("the Go code for this needs the Go type of {what}"),
        )
        .with_help("The checker should have rejected this program. Please report it as a bug")
}

pub fn reserved_go_qualifier(name: &str, span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reserved Go name")
        .with_emit_code("reserved_go_qualifier")
        .with_span_primary_label(span, "reserved for generated imports")
        .with_help(format!(
            "Rename the type. `{}` is the qualifier of a Go package that generated \
             code may import implicitly.",
            name
        ))
}

pub fn go_import_collision(
    alias: &str,
    first: &str,
    second: &str,
    rest: &[&str],
) -> LisetteDiagnostic {
    let mut sorted = vec![first, second];
    sorted.extend_from_slice(rest);
    sorted.sort();

    let packages: Vec<String> = sorted.iter().map(|p| format!("`go:{}`", p)).collect();
    let suggestion_target = sorted[sorted.len() - 1];

    LisetteDiagnostic::error("Go import collision")
        .with_emit_code("go_import_collision")
        .with_help(format!(
            "Add an alias to at least one of {} in your source: \
             `import my_{} \"go:{}\"`. \
             Otherwise they {} default to `{}` in generated code. \
             One of these may have been pulled in transitively by a typedef.",
            pattern::join_and(&packages),
            alias,
            suggestion_target,
            if packages.len() == 2 { "both" } else { "all" },
            alias,
        ))
}
