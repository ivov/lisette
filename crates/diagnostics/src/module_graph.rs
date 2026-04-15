use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub fn module_not_found(
    module_name: &str,
    span: Span,
    is_go_stdlib: bool,
    standalone: bool,
    src_prefix_hint: Option<String>,
) -> LisetteDiagnostic {
    let help = if let Some(stripped) = src_prefix_hint {
        format!(
            "Did you mean `import \"{}\"`? The `src/` prefix is not needed — imports are relative to the source directory.",
            stripped
        )
    } else if is_go_stdlib {
        format!(
            "No `{}` module found in your local project. Did you mean `import \"go:{}\"` from Go's stdlib?",
            module_name, module_name
        )
    } else if standalone {
        "When executing `lis run` on an individual file, that file may import only from the Go standard library. To import modules normally, use `lis new` to create a project."
            .to_string()
    } else {
        "Check the module path and ensure the file exists".to_string()
    };

    LisetteDiagnostic::error("Module not found")
        .with_resolve_code("module_not_found")
        .with_span_label(&span, "not found")
        .with_help(help)
}

pub fn invalid_module_path(module_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("Invalid module path `{}`", module_name))
        .with_resolve_code("invalid_module_path")
        .with_span_label(&span, "module paths cannot contain `.`")
        .with_help(
            "Project imports use bare folder names like `import \"util\"` or `import \"nested/deep/module\"`. Relative-path syntax (`./sub`, `../sub`) is not supported.",
        )
}

pub fn cannot_import_prelude(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_prelude")
        .with_span_label(&span, "prelude is automatically available")
        .with_help("Remove this import. Use e.g. `Option` or `prelude.Option` directly.")
}

pub fn test_file_not_supported(filename: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("Test file `{}` is not yet supported", filename))
        .with_resolve_code("test_file_not_supported")
        .with_help("Files ending in `_test.lis` are reserved for future testing support. Rename this file to compile it.")
}

pub fn undeclared_go_import(go_pkg: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared Go dependency")
        .with_resolve_code("undeclared_go_import")
        .with_span_label(&span, "not in lisette.toml")
        .with_help(format!(
            "Run `lis add {}` to add this dependency, or add it manually to `[dependencies.go]` in `lisette.toml`",
            go_pkg
        ))
}

pub fn missing_go_typedef(
    go_pkg: &str,
    module: &str,
    version: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing Go typedef")
        .with_resolve_code("missing_go_typedef")
        .with_span_label(&span, "no .d.lis file found")
        .with_help(format!(
            "Package `{}` is declared via `{}` {} but no typedef was found. Run `lis check` to regenerate all typedefs, or `lis add {}@{}` to regenerate this one.",
            go_pkg, module, version, module, version
        ))
}

pub fn unreadable_go_typedef(path: &std::path::Path, error: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Failed to read Go typedef")
        .with_resolve_code("unreadable_go_typedef")
        .with_span_label(&span, "typedef exists but could not be read")
        .with_help(format!("Failed to read `{}`: {}", path.display(), error,))
}

pub fn import_cycle(path: &[String]) -> LisetteDiagnostic {
    let modules: Vec<_> = path[..path.len() - 1].to_vec();

    let is_self_import = modules.len() == 1;

    let chain = if is_self_import {
        format!("{} -> {}", modules[0], modules[0])
    } else {
        modules.join(" -> ")
    };

    let first_module = &modules[0];
    let first_end = first_module.len();
    let first_center = first_module.len() / 2;

    let last_module = if is_self_import {
        &modules[0]
    } else {
        modules.last().expect("cycle must have at least one module")
    };
    let last_start = chain.len() - last_module.len();
    let last_end = chain.len();
    let last_center = last_start + last_module.len() / 2;

    let mut underline = String::new();
    for i in 0..chain.len() {
        if i < first_end {
            if i == first_center {
                underline.push('┬');
            } else {
                underline.push('─');
            }
        } else if i >= last_start && i < last_end {
            if i == last_center {
                underline.push('┬');
            } else {
                underline.push('─');
            }
        } else {
            underline.push(' ');
        }
    }

    let mut connect_line = String::new();
    for i in 0..=last_center {
        if i < first_center {
            connect_line.push(' ');
        } else if i == first_center {
            connect_line.push('╰');
        } else if i < last_center {
            connect_line.push('─');
        } else {
            connect_line.push('╯');
        }
    }

    let art = format!("{}\n{}\n{}", chain, underline, connect_line);

    let help = if is_self_import {
        "Remove the self-import"
    } else {
        "To break the cycle, remove one of imports or extract common dependencies into a separate module"
    };

    LisetteDiagnostic::error(format!("Import cycle detected\n\n{}", art))
        .with_resolve_code("import_cycle")
        .with_help(help)
}
