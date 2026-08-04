use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub enum MissingPackageReason {
    NotFound,
    GoStandardLibrary,
    Script { inside_project: bool },
    UnnecessarySrcPrefix(String),
}

pub fn package_not_found(
    package_name: &str,
    span: Span,
    reason: MissingPackageReason,
) -> LisetteDiagnostic {
    let help = match reason {
        MissingPackageReason::UnnecessarySrcPrefix(stripped) => format!(
            "Did you mean `import \"{}\"`? The `src/` prefix is not needed — imports are relative to the source directory.",
            stripped
        ),
        MissingPackageReason::GoStandardLibrary => format!(
            "No `{}` package found in your local project. Did you mean `import \"go:{}\"` from Go's stdlib?",
            package_name, package_name
        ),
        MissingPackageReason::Script {
            inside_project: false,
        } => {
            "A file compiled on its own may import only from the Go standard library. To import packages normally, use `lis new` to create a project."
                .to_string()
        }
        MissingPackageReason::Script {
            inside_project: true,
        } => {
            "A file compiled on its own may import only from the Go standard library. This file sits inside a project but outside its `src/`, so it is not part of it. Move it under `src/` to import the project's packages."
                .to_string()
        }
        MissingPackageReason::NotFound => {
            "Check the package path and ensure the file exists".to_string()
        }
    };

    LisetteDiagnostic::error("Package not found")
        .with_resolve_code("package_not_found")
        .with_span_label(&span, "not a package in this project")
        .with_help(help)
}

/// A path with a dotted first segment and nonempty following segments, like `github.com/gorilla/mux`.
pub fn is_go_package_shaped(package_name: &str) -> bool {
    let Some((first_segment, rest)) = package_name.split_once('/') else {
        return false;
    };
    first_segment.contains('.')
        && !matches!(first_segment, "." | "..")
        && rest.split('/').all(|segment| !segment.is_empty())
}

pub fn invalid_package_path(package_name: &str, span: Span, is_blank: bool) -> LisetteDiagnostic {
    let help = if is_go_package_shaped(package_name) {
        let suggestion = if is_blank {
            format!("import _ \"go:{}\"", package_name)
        } else {
            format!("import \"go:{}\"", package_name)
        };
        let add_argument = package_name
            .strip_prefix("github.com/")
            .unwrap_or(package_name);
        format!(
            "Go packages are imported with the `go:` prefix: `{}`. Add it first with `lis add {}`",
            suggestion, add_argument
        )
    } else {
        "Project imports use bare folder names like `import \"util\"` or `import \"nested/deep/package\"`. Relative-path syntax (`./sub`, `../sub`) is not supported.".to_string()
    };

    LisetteDiagnostic::error(format!("Invalid package path `{}`", package_name))
        .with_resolve_code("invalid_package_path")
        .with_span_label(&span, "package paths cannot contain `.`")
        .with_help(help)
}

pub fn dotted_package_directory(package_id: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("Dotted package directory `{}`", package_id))
        .with_resolve_code("dotted_package_directory")
        .with_help(
            "Rename the directory. `.` separates a package path from the name it qualifies, as in `v1.VConf`, so a package path cannot contain one.",
        )
}

pub fn missing_go_prefix(package_name: &str, span: Span, is_blank: bool) -> LisetteDiagnostic {
    let suggestion = if is_blank {
        format!("import _ \"go:{}\"", package_name)
    } else {
        format!("import \"go:{}\"", package_name)
    };
    LisetteDiagnostic::error(format!("Invalid package path `{}`", package_name))
        .with_resolve_code("missing_go_prefix")
        .with_span_label(&span, "Go imports require the `go:` prefix")
        .with_help(format!(
            "`{}` is a declared Go dependency. Did you mean `{}`?",
            package_name, suggestion
        ))
}

pub fn cannot_import_prelude(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_prelude")
        .with_span_label(&span, "prelude is automatically available")
        .with_help("Remove this import. Use e.g. `Option` or `prelude.Option` directly.")
}

pub fn reserved_package_import(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("reserved_package_import")
        .with_span_label(&span, "the `**` prefix is reserved for the compiler")
        .with_help("Rename the package so its import path does not begin with `**`.")
}

pub fn cannot_import_external_tests(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_external_tests")
        .with_span_label(&span, "reserved package name")
        .with_help("The `tests` package at project root is reserved for external tests")
}

pub fn cannot_import_entry(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "reserved package name")
        .with_help("`_entry_` is an internal package. Import a library's root package with `root`")
}

pub fn cannot_import_root_from_src(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "reserved package name")
        .with_help("`root` names the library's root package and is importable only from external tests under `tests/`")
}

pub fn cannot_import_root_in_binary(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "reserved package name")
        .with_help("A binary has no importable root. Move testable code into a sub-package under `src/`, or make the project a library")
}

pub fn cannot_import_root_without_source(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("cannot_import_root")
        .with_span_label(&span, "no root package")
        .with_help("This library has no root package because `src/` holds no source files directly. Import a sub-package by name, or add a source file under `src/`")
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
    local_package: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared Go dependency")
        .with_resolve_code("undeclared_go_import")
        .with_span_label(&span, "not in lisette.toml")
        .with_help(format!(
            "`{}` is a dependency of the local module `{}`. Run `lis sync` if it is published, or `lis add --path <dir>` if `{}` resolves it from a local directory",
            go_pkg, local_package, local_package
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

pub fn unreachable_package(package_id: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn(format!("Unreachable package: `{}`", package_id))
        .with_resolve_code("unreachable_package")
        .with_help("This package is never imported. Use or remove it.")
}

pub struct CycleHop<'a> {
    pub package: &'a str,
    pub span: Span,
}

pub fn import_cycle(hops: &[CycleHop<'_>]) -> LisetteDiagnostic {
    let is_self_import = hops.len() == 1;

    let help = if is_self_import {
        "Remove the self-import"
    } else {
        "To break the cycle, remove one of the imports or extract common dependencies into a separate package"
    };

    let chain: Vec<&str> = hops
        .iter()
        .map(|hop| hop.package)
        .chain(hops.first().map(|hop| hop.package))
        .collect();

    let mut diagnostic =
        LisetteDiagnostic::error(format!("Import cycle detected: {}", chain.join(" -> ")))
            .with_resolve_code("import_cycle")
            .with_help(help);

    for (index, hop) in hops.iter().enumerate() {
        let label = if is_self_import {
            format!("`{}` imports itself", hop.package)
        } else {
            format!(
                "`{}` imports `{}`",
                hop.package,
                hops[(index + 1) % hops.len()].package
            )
        };
        diagnostic = if index == 0 {
            diagnostic.with_span_primary_label(&hop.span, label)
        } else {
            diagnostic.with_span_label(&hop.span, label)
        };
    }

    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_package_path_suggests_go_import_for_go_shaped_path() {
        let diagnostic = invalid_package_path("github.com/gorilla/mux", Span::new(0, 0, 1), false);
        let help = diagnostic.plain_help().unwrap_or_default();
        assert!(help.contains("import \"go:github.com/gorilla/mux\""));
        assert!(help.contains("lis add gorilla/mux"));
    }

    #[test]
    fn invalid_package_path_preserves_blank_alias_in_suggestion() {
        let diagnostic = invalid_package_path("github.com/gorilla/mux", Span::new(0, 0, 1), true);
        let help = diagnostic.plain_help().unwrap_or_default();
        assert!(
            help.contains("import _ \"go:github.com/gorilla/mux\""),
            "help was: {}",
            help
        );
    }

    #[test]
    fn invalid_package_path_keeps_full_path_for_non_github_host() {
        let diagnostic = invalid_package_path("go.uber.org/zap", Span::new(0, 0, 1), false);
        let help = diagnostic.plain_help().unwrap_or_default();
        assert!(help.contains("import \"go:go.uber.org/zap\""));
        assert!(help.contains("lis add go.uber.org/zap"));
    }

    #[test]
    fn go_package_shape_requires_nonempty_segments() {
        assert!(is_go_package_shaped("github.com/gorilla/mux"));
        assert!(!is_go_package_shaped("github.com/"));
        assert!(!is_go_package_shaped("github.com//foo"));
        assert!(!is_go_package_shaped("github.com/foo/"));
    }

    #[test]
    fn invalid_package_path_keeps_folder_help_for_relative_path() {
        for path in ["./sub", "../sub"] {
            let diagnostic = invalid_package_path(path, Span::new(0, 0, 1), false);
            let help = diagnostic.plain_help().unwrap_or_default();
            assert!(help.contains("Relative-path syntax"), "help was: {}", help);
        }
    }

    #[test]
    fn unreachable_package_names_one_package_and_carries_a_code() {
        let diagnostic = unreachable_package("orphan");
        assert!(diagnostic.is_warning());
        assert_eq!(diagnostic.plain_message(), "Unreachable package: `orphan`");
        assert_eq!(diagnostic.code_str(), Some("resolve.unreachable_package"));
    }

    fn hop(package: &str, file_id: u32) -> CycleHop<'_> {
        CycleHop {
            package,
            span: Span::new(file_id, 7, 6),
        }
    }

    #[test]
    fn import_cycle_names_the_whole_chain_on_one_line() {
        let diagnostic = import_cycle(&[hop("alpha", 1), hop("beta", 2)]);

        assert_eq!(
            diagnostic.plain_message(),
            "Import cycle detected: alpha -> beta -> alpha"
        );
        assert!(!diagnostic.plain_message().contains('\n'));
    }

    #[test]
    fn import_cycle_labels_the_import_of_every_hop() {
        let diagnostic = import_cycle(&[hop("alpha", 1), hop("beta", 2)]);

        assert_eq!(diagnostic.label_file_ids(), vec![1, 2]);
        assert_eq!(diagnostic.plain_label(), Some("`alpha` imports `beta`"));
        assert_eq!(diagnostic.file_id(), Some(1));
    }

    #[test]
    fn import_cycle_calls_out_a_self_import() {
        let diagnostic = import_cycle(&[hop("alpha", 1)]);

        assert_eq!(
            diagnostic.plain_message(),
            "Import cycle detected: alpha -> alpha"
        );
        assert_eq!(diagnostic.plain_help(), Some("Remove the self-import"));
        assert_eq!(diagnostic.plain_label(), Some("`alpha` imports itself"));
    }
}
