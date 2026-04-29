use crate::LisetteDiagnostic;

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
