use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub enum MissingModuleReason {
    NotFound,
    GoStandardLibrary,
    Standalone { inside_project: bool },
    UnnecessarySrcPrefix(String),
}

pub fn module_not_found(
    module_name: &str,
    span: Span,
    reason: MissingModuleReason,
) -> LisetteDiagnostic {
    let help = match reason {
        MissingModuleReason::UnnecessarySrcPrefix(stripped) => format!(
            "Did you mean `import \"{}\"`? The `src/` prefix is not needed — imports are relative to the source directory.",
            stripped
        ),
        MissingModuleReason::GoStandardLibrary => format!(
            "No `{}` module found in your local project. Did you mean `import \"go:{}\"` from Go's stdlib?",
            module_name, module_name
        ),
        MissingModuleReason::Standalone {
            inside_project: false,
        } => {
            "A file compiled on its own may import only from the Go standard library. To import modules normally, use `lis new` to create a project."
                .to_string()
        }
        MissingModuleReason::Standalone {
            inside_project: true,
        } => {
            "A file compiled on its own may import only from the Go standard library. This file sits inside a project but outside its `src/`, so it is not part of it. Move it under `src/` to import the project's modules."
                .to_string()
        }
        MissingModuleReason::NotFound => {
            "Check the module path and ensure the file exists".to_string()
        }
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

pub fn missing_go_prefix(module_name: &str, span: Span, is_blank: bool) -> LisetteDiagnostic {
    let suggestion = if is_blank {
        format!("import _ \"go:{}\"", module_name)
    } else {
        format!("import \"go:{}\"", module_name)
    };
    LisetteDiagnostic::error(format!("Invalid module path `{}`", module_name))
        .with_resolve_code("missing_go_prefix")
        .with_span_label(&span, "Go imports require the `go:` prefix")
        .with_help(format!(
            "`{}` is a declared Go dependency. Did you mean `{}`?",
            module_name, suggestion
        ))
}

pub fn cannot_import_prelude(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_prelude")
        .with_span_label(&span, "prelude is automatically available")
        .with_help("Remove this import. Use e.g. `Option` or `prelude.Option` directly.")
}

pub fn reserved_module_import(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("reserved_module_import")
        .with_span_label(&span, "the `**` prefix is reserved for the compiler")
        .with_help("Rename the module so its import path does not begin with `**`.")
}

pub fn cannot_import_external_tests(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_external_tests")
        .with_span_label(&span, "reserved module name")
        .with_help("The `tests` module at project root is reserved for external tests")
}

pub fn cannot_import_entry(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "reserved module name")
        .with_help("`_entry_` is an internal module. Import a library's root package with `root`")
}

pub fn cannot_import_root_from_src(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "reserved module name")
        .with_help("`root` names the library's root package and is importable only from external tests under `tests/`")
}

pub fn cannot_import_root_in_binary(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "reserved module name")
        .with_help("A binary has no importable root. Move testable code into a sub-module under `src/`, or make the project a library")
}

pub fn cannot_import_root_without_source(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "no root package")
        .with_help("This library has no root package because `src/` holds no source files directly. Import a sub-module by name, or add a source file under `src/`")
}

pub fn wrong_test_file_suffix(display_path: &str) -> LisetteDiagnostic {
    let help = match display_path.strip_suffix("_test.lis") {
        Some(stem) => format!(
            "Lisette test files use the `.test.lis` suffix. Rename this file to `{}.test.lis`.",
            stem
        ),
        None => "Lisette test files use the `.test.lis` suffix.".to_string(),
    };

    LisetteDiagnostic::error(format!(
        "Test file `{}` has an unsupported suffix",
        display_path
    ))
    .with_resolve_code("wrong_test_file_suffix")
    .with_file_location(display_path)
    .with_help(help)
}

pub fn non_test_file_under_tests(display_path: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("`{}` is not a test file", display_path))
        .with_resolve_code("non_test_file_under_tests")
        .with_file_location(display_path)
        .with_help("Files under `tests/` must use the `.test.lis` suffix. Rename this file or move it into `src/`")
}

pub fn cannot_emit_test_file(display_path: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!(
        "Test file `{}` cannot be built or run as a program",
        display_path
    ))
    .with_resolve_code("cannot_emit_test_file")
    .with_file_location(display_path)
    .with_help("Test files are not entry points. Use `lis check` to type-check this file.")
}

pub fn go_stdlib_unavailable_on_target(
    go_pkg: &str,
    target: &str,
    available: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("`go:{}` is not available on `{}`", go_pkg, target))
        .with_resolve_code("go_stdlib_unavailable_on_target")
        .with_span_label(&span, "stdlib package not available on this target")
        .with_help(format!(
            "This Go stdlib package exists, but its surface differs across platforms. Available on: {}",
            available
        ))
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

pub fn undeclared_go_import_via_replace(
    go_pkg: &str,
    replaced_module: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared Go dependency")
        .with_resolve_code("undeclared_go_import")
        .with_span_label(&span, "not in lisette.toml")
        .with_help(format!(
            "`{}` is a dependency of the replaced module `{}`. Run `lis sync` to reconcile the replacement's dependencies, or `lis add {}` to add it directly",
            go_pkg, replaced_module, go_pkg
        ))
}

pub fn undeclared_go_import_via_local(
    go_pkg: &str,
    local_module: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared Go dependency")
        .with_resolve_code("undeclared_go_import")
        .with_span_label(&span, "not in lisette.toml")
        .with_help(format!(
            "`{}` is a dependency of the local module `{}`. Run `lis sync` if it is published, or `lis add --path <dir>` if `{}` resolves it from a local directory",
            go_pkg, local_module, local_module
        ))
}

pub fn internal_go_package(go_pkg: &str, module: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Internal Go package")
        .with_resolve_code("internal_go_package")
        .with_span_label(&span, "not importable outside its module")
        .with_help(format!(
            "`{}` is an `internal` package of `{}`. Go forbids importing internal packages across module boundaries. Use the module's public API instead",
            go_pkg, module
        ))
}

pub fn missing_local_go_typedef(module: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing Go typedef")
        .with_resolve_code("missing_go_typedef")
        .with_span_label(&span, "no .d.lis file found")
        .with_help(format!(
            "Module `{}` is sourced from a local directory but has no typedef. Run `lis sync` to regenerate it.",
            module
        ))
}

pub fn missing_go_typedef(
    go_pkg: &str,
    module: &str,
    version: &str,
    replacement_path: Option<&str>,
    span: Span,
) -> LisetteDiagnostic {
    let help = if let Some(replacement_path) = replacement_path {
        format!(
            "Module `{}` is sourced via `replace` from `{}@{}` but has no typedef. Run `lis sync` to regenerate it.",
            module, replacement_path, version
        )
    } else if go_pkg == module {
        format!(
            "Module `{}` {} is declared but no typedef was found. Run `lis check` to regenerate all typedefs, or `lis add {}@{}` to regenerate this one.",
            module, version, module, version
        )
    } else {
        format!(
            "Subpackage `{}` of module `{}` {} has no typedef. Run `lis add {}@{}` to regenerate the module's typedefs, including any subpackages.",
            go_pkg, module, version, module, version
        )
    };

    LisetteDiagnostic::error("Missing Go typedef")
        .with_resolve_code("missing_go_typedef")
        .with_span_label(&span, "no .d.lis file found")
        .with_help(help)
}

pub fn unreadable_go_typedef(path: &std::path::Path, error: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Failed to read Go typedef")
        .with_resolve_code("unreadable_go_typedef")
        .with_span_label(&span, "typedef exists but could not be read")
        .with_help(format!("Failed to read `{}`: {}", path.display(), error,))
}

pub fn go_toolchain_missing(go_pkg: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!(
        "Cannot generate Go typedef for `{}`: `go` is not installed",
        go_pkg
    ))
    .with_resolve_code("go_toolchain_missing")
    .with_span_label(&span, "needs the Go toolchain")
    .with_help("Install Go from https://go.dev/dl/")
}

pub fn bindgen_failed(
    go_pkg: &str,
    module: &str,
    version: &str,
    stderr: &str,
    span: Span,
) -> LisetteDiagnostic {
    let trimmed = stderr.trim();
    let stderr_block = if trimmed.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", trimmed)
    };

    LisetteDiagnostic::error(format!(
        "Failed to generate Go typedef for `{}` ({} {})",
        go_pkg, module, version
    ))
    .with_resolve_code("bindgen_failed")
    .with_span_label(&span, "bindgen failed for this import")
    .with_help(format!(
        "Re-run with `lis bindgen {}` to inspect the failure in isolation.{}",
        go_pkg, stderr_block
    ))
}

pub fn unreachable_module(module_id: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn(format!("Unreachable module: `{}`", module_id))
        .with_resolve_code("unreachable_module")
        .with_help("This module is never imported. Use or remove it.")
}

pub fn import_cycle(path: &[String]) -> LisetteDiagnostic {
    let is_self_import = path.len() == 2;

    let help = if is_self_import {
        "Remove the self-import"
    } else {
        "To break the cycle, remove one of the imports or extract common dependencies into a separate module"
    };

    LisetteDiagnostic::error(format!("Import cycle detected: {}", path.join(" -> ")))
        .with_resolve_code("import_cycle")
        .with_help(help)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_module_names_one_module_and_carries_a_code() {
        let diagnostic = unreachable_module("orphan");
        assert!(diagnostic.is_warning());
        assert_eq!(diagnostic.plain_message(), "Unreachable module: `orphan`");
        assert_eq!(diagnostic.code_str(), Some("resolve.unreachable_module"));
    }

    #[test]
    fn import_cycle_names_the_whole_chain_on_one_line() {
        let cycle = ["alpha".to_string(), "beta".to_string(), "alpha".to_string()];

        let diagnostic = import_cycle(&cycle);

        assert_eq!(
            diagnostic.plain_message(),
            "Import cycle detected: alpha -> beta -> alpha"
        );
        assert!(!diagnostic.plain_message().contains('\n'));
    }

    #[test]
    fn import_cycle_calls_out_a_self_import() {
        let cycle = ["alpha".to_string(), "alpha".to_string()];

        let diagnostic = import_cycle(&cycle);

        assert_eq!(
            diagnostic.plain_message(),
            "Import cycle detected: alpha -> alpha"
        );
        assert_eq!(diagnostic.plain_help(), Some("Remove the self-import"));
    }
}
