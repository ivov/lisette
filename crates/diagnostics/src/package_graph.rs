use crate::LisetteDiagnostic;
use std::path::Path;
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
            "Did you mean `import \"{}\"`? The `src/` prefix is not needed. Imports are relative to the source directory.",
            stripped
        ),
        MissingPackageReason::GoStandardLibrary => format!(
            "No `{}` package found in your local project. Did you mean `import \"go:{}\"` from Go's standard library?",
            package_name, package_name
        ),
        MissingPackageReason::Script {
            inside_project: false,
        } => {
            "Use `lis new` to create a project. A file compiled on its own has no packages beside it, only the Go ones it imports."
                .to_string()
        }
        MissingPackageReason::Script {
            inside_project: true,
        } => {
            "Move this file under `src/` to import the project's packages. This file sits inside a project but outside its `src/`, so it is not part of it."
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

pub fn invalid_package_path(
    package_name: &str,
    span: Span,
    is_blank: bool,
    script: bool,
) -> LisetteDiagnostic {
    let help = if is_go_package_shaped(package_name) {
        let blank = if is_blank { "_ " } else { "" };
        if script {
            format!(
                "Import the package with the `go:` prefix, `import {}\"go:{}\"`, and declare the module that provides it in the `[dependencies.go]` table",
                blank, package_name
            )
        } else {
            format!(
                "Import the package with the `go:` prefix, `import {}\"go:{}\"`, after you add it with `lis add {}`",
                blank,
                package_name,
                package_name
                    .strip_prefix("github.com/")
                    .unwrap_or(package_name)
            )
        }
    } else {
        "Project imports use bare folder names, e.g. `import \"util\"` or `import \"nested/deep/package\"`. Relative-path syntax (`./sub`, `../sub`) is not supported.".to_string()
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
            "Rename the directory. `.` separates a package path from the name it qualifies, e.g. `v1.VConf`, so a package path cannot contain one.",
        )
}

pub fn missing_go_prefix(package_name: &str, span: Span, is_blank: bool) -> LisetteDiagnostic {
    let suggestion = if is_blank {
        format!("import _ \"go:{}\"", package_name)
    } else {
        format!("import \"go:{}\"", package_name)
    };
    LisetteDiagnostic::error("Missing `go:` prefix")
        .with_resolve_code("missing_go_prefix")
        .with_span_label(&span, "Go imports require the `go:` prefix")
        .with_help(format!(
            "`{}` is a declared Go dependency. Did you mean `{}`?",
            package_name, suggestion
        ))
}

pub fn cannot_import_prelude(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unnecessary prelude import")
        .with_resolve_code("cannot_import_prelude")
        .with_span_label(&span, "prelude is automatically available")
        .with_help("Remove this import. Use e.g. `Option` or `prelude.Option` directly.")
}

pub fn prelude_in_go_api(importer: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unsupported Go dependency")
        .with_resolve_code("prelude_in_go_api")
        .with_span_label(&span, "not supported yet")
        .with_help(format!(
            "`{importer}` uses Lisette's `Option` and `Result` types in its Go API, and Lisette cannot use such an API yet"
        ))
}

pub fn reserved_package_import(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reserved package path")
        .with_resolve_code("reserved_package_import")
        .with_span_label(&span, "the `**` prefix is reserved for the compiler")
        .with_help("Rename the package so its import path does not begin with `**`.")
}

pub fn cannot_import_external_tests(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unimportable `tests` package")
        .with_resolve_code("cannot_import_external_tests")
        .with_span_label(&span, "reserved package name")
        .with_help("The `tests` package at project root is reserved for external tests")
}

pub fn cannot_import_entry(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unimportable internal package")
        .with_resolve_code("cannot_import_entry")
        .with_span_label(&span, "reserved package name")
        .with_help("Import a library's root package with `root`. `_entry_` is an internal package")
}

pub fn cannot_import_root_from_src(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`root` import outside tests")
        .with_resolve_code("cannot_import_root_from_src")
        .with_span_label(&span, "reserved package name")
        .with_help("`root` names the library's root package and is importable only from external tests under `tests/`")
}

pub fn cannot_import_root_in_binary(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`root` import in a binary")
        .with_resolve_code("cannot_import_root_in_binary")
        .with_span_label(&span, "reserved package name")
        .with_help("Move testable code into a sub-package under `src/`, or make the project a library. A binary has no importable root")
}

pub fn cannot_import_root_without_source(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing root package")
        .with_resolve_code("cannot_import_root_without_source")
        .with_span_label(&span, "no root package")
        .with_help("Import a sub-package by name, or add a source file under `src/`. This library has no root package because `src/` holds no source files directly")
}

pub fn wrong_test_file_suffix(display_path: &str) -> LisetteDiagnostic {
    let help = match display_path.strip_suffix("_test.lis") {
        Some(stem) => format!(
            "Rename this file to `{}.test.lis`. Lisette test files use the `.test.lis` suffix.",
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
        .with_help("Rename this file or move it into `src/`. Files under `tests/` must use the `.test.lis` suffix")
}

pub fn cannot_emit_test_file(display_path: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!(
        "Test file `{}` cannot be built or run as a program",
        display_path
    ))
    .with_resolve_code("cannot_emit_test_file")
    .with_file_location(display_path)
    .with_help("Use `lis check` to type-check this file. Test files are not entry points.")
}

pub fn go_stdlib_unavailable_on_target(
    go_pkg: &str,
    target: &str,
    available: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("`go:{}` is not available on `{}`", go_pkg, target))
        .with_resolve_code("go_stdlib_unavailable_on_target")
        .with_span_label(&span, "standard library package not available on this target")
        .with_help(format!(
            "This Go standard library package exists, but its surface differs across platforms. Available on: {}",
            available
        ))
}

pub fn unknown_go_stdlib_package(go_pkg: &str, span: Span, script: bool) -> LisetteDiagnostic {
    let third_party = if script {
        "a third-party one needs its full module path, and an entry in the `[dependencies.go]` table"
    } else {
        "a third-party one needs its full module path, and `lis add` to declare it"
    };

    LisetteDiagnostic::error(format!("`{}` is not a Go standard library package", go_pkg))
        .with_resolve_code("unknown_go_stdlib_package")
        .with_span_label(&span, "no such package in the Go standard library")
        .with_help(format!("Check the spelling: {}", third_party))
}

pub fn empty_import_path(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Empty import path")
        .with_resolve_code("empty_import_path")
        .with_span_label(&span, "no package named")
        .with_help(
            "Name the package after `go:`, as `import \"go:fmt\"` or `import \"go:github.com/google/uuid\"`",
        )
}

pub fn invalid_go_package_path(go_pkg: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("Invalid Go package path `{}`", go_pkg))
        .with_resolve_code("invalid_go_package_path")
        .with_span_label(&span, "not a path Go can resolve")
        .with_help(
            "Write the path as Go does, with no empty, `.`, or `..` segments, e.g. `github.com/google/uuid`",
        )
}

pub fn invalid_dependency_table(message: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid dependency table")
        .with_resolve_code("invalid_dependency_table")
        .with_span_label(&span, "not a valid entry")
        .with_help(format!(
            "{}. The table holds one Go module per line, as `\"github.com/google/uuid\" = \"v1.6.0\"`",
            message.trim_end_matches('.')
        ))
}

pub fn duplicate_dependency_table(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Duplicate dependency table")
        .with_resolve_code("duplicate_dependency_table")
        .with_span_label(&span, "a second `[dependencies.go]` table")
        .with_help("Merge the entries into the first table and delete this one")
}

pub fn dependency_table_in_project(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Dependency table in a project file")
        .with_resolve_code("dependency_table_in_project")
        .with_span_label(&span, "not read here")
        .with_help(
            "Move these entries to `lisette.toml`, where `lis add` and `lis sync` maintain them. A project records dependencies in `lisette.toml`",
        )
}

pub fn script_undeclared_dependency(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared Go dependency")
        .with_resolve_code("script_undeclared_dependency")
        .with_span_label(&span, "not in this script's dependency table")
        .with_help("Declare the module that provides it in the `[dependencies.go]` table")
}

pub fn script_transitive_go_dependency(
    go_pkg: &str,
    importer: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared transitive Go dependency")
        .with_resolve_code("script_transitive_go_dependency")
        .with_span_label(&span, "not in this script's dependency table")
        .with_help(format!(
            "`{}` appears in `{}`'s API, so the `[dependencies.go]` table has to declare its module too",
            go_pkg, importer
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

pub fn undeclared_transitive_go_import(
    go_pkg: &str,
    importer: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undeclared Go dependency")
        .with_resolve_code("undeclared_go_import")
        .with_span_label(&span, "not in lisette.toml")
        .with_help(format!(
            "Run `lis sync` to declare the modules that `{}` needs, or `lis add {}` to add this dependency directly",
            importer, go_pkg
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
            "Run `lis sync` to reconcile the replacement's dependencies, or `lis add {}` to add it directly. `{}` is a dependency of the replaced module `{}`",
            go_pkg, go_pkg, replaced_module
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
            "Run `lis sync` if `{}` is published, or `lis add --path <dir>` if `{}` resolves it from a local directory. `{}` is a dependency of the local module `{}`",
            go_pkg, local_package, go_pkg, local_package
        ))
}

pub fn internal_go_package(go_pkg: &str, module: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Internal Go package")
        .with_resolve_code("internal_go_package")
        .with_span_label(&span, "not importable outside its module")
        .with_help(format!(
            "Use the public API of `{}`. `{}` is an `internal` package of that module, and Go forbids importing internal packages across module boundaries",
            module, go_pkg
        ))
}

pub fn missing_local_go_typedef(module: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing Go typedef")
        .with_resolve_code("missing_go_typedef")
        .with_span_label(&span, "no .d.lis file found")
        .with_help(format!(
            "Run `lis sync` to regenerate the typedef. Module `{}` is sourced from a local directory but has no typedef.",
            module
        ))
}

pub fn missing_go_typedef(
    go_pkg: &str,
    module: &str,
    version: &str,
    replacement_path: Option<&str>,
    span: Span,
    script: bool,
) -> LisetteDiagnostic {
    let help = if let Some(replacement_path) = replacement_path {
        format!(
            "Run `lis sync` to regenerate the typedef. Module `{}` is sourced via `replace` from `{}@{}` but has no typedef.",
            module, replacement_path, version
        )
    } else if script {
        format!(
            "Run `lis run` or `lis build` on this file to generate the typedef. Module `{}` {} resolved but its typedef is missing.",
            module, version
        )
    } else if go_pkg == module {
        format!(
            "Run `lis check` to regenerate all typedefs, or `lis add {}@{}` to regenerate this one. Module `{}` {} is declared but no typedef was found.",
            module, version, module, version
        )
    } else {
        format!(
            "Run `lis add {}@{}` to regenerate the module's typedefs, including any subpackages. Subpackage `{}` of module `{}` {} has no typedef.",
            module, version, go_pkg, module, version
        )
    };

    LisetteDiagnostic::error("Missing Go typedef")
        .with_resolve_code("missing_go_typedef")
        .with_span_label(&span, "no .d.lis file found")
        .with_help(help)
}

pub fn corrupt_go_typedef(go_pkg: &str, discarded: bool, script: bool) -> LisetteDiagnostic {
    let regenerate = if script {
        "Run `lis run` or `lis build` on this file again to regenerate it"
    } else {
        "Run `lis sync` to regenerate it"
    };
    let help = if discarded {
        format!("{}. The cached typedef has been discarded.", regenerate)
    } else {
        format!("Delete the cached typedef. {}", regenerate)
    };

    LisetteDiagnostic::error(format!("Corrupt Go typedef for `{}`", go_pkg))
        .with_resolve_code("corrupt_go_typedef")
        .with_help(help)
}

pub fn unreadable_go_typedef(path: &Path, error: &str, span: Span) -> LisetteDiagnostic {
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

fn indent_reported_output(reported: &str) -> String {
    let mut block = String::from("bindgen reported:");
    for line in reported.lines() {
        let expanded = line.replace('\t', "    ");
        block.push_str("\n  ");
        block.push_str(expanded.trim_end());
    }
    block
}

pub struct BindgenFailureDetails<'a> {
    pub go_pkg: &'a str,
    pub module: &'a str,
    pub version: &'a str,
    pub stderr: &'a str,
    pub hint: Option<&'a str>,
    pub script: bool,
}

pub fn bindgen_failed(details: BindgenFailureDetails<'_>, span: Span) -> LisetteDiagnostic {
    let BindgenFailureDetails {
        go_pkg,
        module,
        version,
        stderr,
        hint,
        script,
    } = details;

    let reported = stderr.trim();
    let advice = match (hint, script) {
        (Some(hint), _) => hint.to_string(),
        (None, true) => String::new(),
        (None, false) => format!(
            "Re-run with `lis bindgen {}` to inspect the failure in isolation.",
            go_pkg
        ),
    };
    let help = match (reported, advice.as_str()) {
        ("", "") => format!("Check that `{}` exists in {} {}", go_pkg, module, version),
        ("", advice) => advice.to_string(),
        (reported, "") if reported.contains('\n') => indent_reported_output(reported),
        (reported, "") => reported.to_string(),
        (reported, advice) if reported.contains('\n') => {
            format!("{}\n{}", advice, indent_reported_output(reported))
        }
        (reported, advice) => format!("{}. {}", reported, advice),
    };

    let origin = if go_pkg == module {
        version.to_string()
    } else {
        format!("{} {}", module, version)
    };

    LisetteDiagnostic::error(format!(
        "Failed to generate Go typedef for `{}` ({})",
        go_pkg, origin
    ))
    .with_resolve_code("bindgen_failed")
    .with_span_label(&span, "bindgen failed for this import")
    .with_help(help)
}

pub fn unreachable_package(package_id: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn(format!("Unreachable package: `{}`", package_id))
        .with_resolve_code("unreachable_package")
        .with_help("Use or remove this package. It is never imported.")
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
        let diagnostic =
            invalid_package_path("github.com/gorilla/mux", Span::new(0, 0, 1), false, false);
        let help = diagnostic.plain_help().unwrap_or_default();
        assert!(help.contains("import \"go:github.com/gorilla/mux\""));
        assert!(help.contains("lis add gorilla/mux"));
    }

    #[test]
    fn invalid_package_path_preserves_blank_alias_in_suggestion() {
        let diagnostic =
            invalid_package_path("github.com/gorilla/mux", Span::new(0, 0, 1), true, false);
        let help = diagnostic.plain_help().unwrap_or_default();
        assert!(
            help.contains("import _ \"go:github.com/gorilla/mux\""),
            "help was: {}",
            help
        );
    }

    #[test]
    fn invalid_package_path_keeps_full_path_for_non_github_host() {
        let diagnostic = invalid_package_path("go.uber.org/zap", Span::new(0, 0, 1), false, false);
        let help = diagnostic.plain_help().unwrap_or_default();
        assert!(help.contains("import \"go:go.uber.org/zap\""));
        assert!(help.contains("lis add go.uber.org/zap"));
    }

    #[test]
    fn an_undeclared_dependency_points_at_the_table() {
        let direct = script_undeclared_dependency(Span::new(0, 0, 1));
        assert!(
            direct
                .plain_help()
                .unwrap_or_default()
                .contains("[dependencies.go]")
        );

        let transitive = script_transitive_go_dependency(
            "github.com/spf13/pflag",
            "github.com/spf13/cobra",
            Span::new(0, 0, 1),
        );
        let help = transitive.plain_help().unwrap_or_default();
        assert!(help.contains("[dependencies.go]"), "help was: {}", help);
        assert!(
            help.contains("github.com/spf13/cobra"),
            "help was: {}",
            help
        );
    }

    #[test]
    fn invalid_package_path_points_a_script_at_its_table() {
        let diagnostic =
            invalid_package_path("github.com/gorilla/mux", Span::new(0, 0, 1), false, true);
        let help = diagnostic.plain_help().unwrap_or_default();

        assert!(help.contains("import \"go:github.com/gorilla/mux\""));
        assert!(help.contains("[dependencies.go]"), "help was: {}", help);
        assert!(!help.contains("lis add"), "help was: {}", help);
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
            let diagnostic = invalid_package_path(path, Span::new(0, 0, 1), false, false);
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
