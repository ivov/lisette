use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub fn go_name_collision(go_name: &str, spans: &[Span], detail: Option<&str>) -> LisetteDiagnostic {
    debug_assert!(spans.len() > 1, "a collision needs at least two sources");
    let mut diagnostic = LisetteDiagnostic::error(format!("Go name collision on `{}`", go_name));
    for (index, span) in spans.iter().enumerate() {
        let label = format!("becomes `{}` in Go", go_name);
        diagnostic = if index == 0 {
            diagnostic.with_span_primary_label(span, label)
        } else {
            diagnostic.with_span_label(span, label)
        };
    }
    let mut help = format!(
        "These declarations all become `{}` in generated Go, but Go requires \
         package-level names to be distinct. Rename all but one.",
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
            "`{}` starts with `{}`, which is reserved for compiler-generated \
             interface adapters. Rename the declaration.",
            name, prefix
        ))
}

pub fn reserved_go_qualifier(name: &str, span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reserved Go name")
        .with_emit_code("reserved_go_qualifier")
        .with_span_primary_label(span, "reserved for generated imports")
        .with_help(format!(
            "`{}` is the qualifier of a Go package that generated code may \
             import implicitly. Rename the type.",
            name
        ))
}

pub fn go_import_collision(alias: &str, paths: &[String]) -> LisetteDiagnostic {
    let mut sorted = paths.to_vec();
    sorted.sort();

    let bullet_list = sorted
        .iter()
        .map(|p| format!("  - go:{}", p))
        .collect::<Vec<_>>()
        .join("\n");

    let suggestion_target = sorted.last().cloned().unwrap_or_default();

    LisetteDiagnostic::error("Go import collision")
        .with_emit_code("go_import_collision")
        .with_help(format!(
            "These Go packages all default to `{}` in generated code:\n{}\n\
             Add an alias to at least one of them in your source: \
             `import my_{} \"go:{}\"`. \
             One of these may have been pulled in transitively by a typedef.",
            alias, bullet_list, alias, suggestion_target,
        ))
}
