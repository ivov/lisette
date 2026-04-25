use std::path::{Path, PathBuf};

use syntax::ast::Expression;
use syntax::parse::Parser;

use lisette::fs::collect_lis_filepaths_recursive;

use crate::handlers::add::find_project_root;
use crate::lock::acquire_mutation_lock;
use crate::output::{print_preview_notice, print_sync_summary};
use crate::{cli_error, error};

pub fn sync() -> i32 {
    let project_root = match find_project_root() {
        Some(root) => root,
        None => {
            cli_error!(
                "No project found",
                "No `lisette.toml` in current directory or in any parent",
                "Run `lis new <name>` to create a project"
            );
            return 1;
        }
    };

    let manifest = match deps::parse_manifest(&project_root) {
        Ok(m) => m,
        Err(msg) => {
            cli_error!("Failed to read manifest", msg, "Fix `lisette.toml`");
            return 1;
        }
    };

    if let Err(msg) = deps::check_toolchain_version(&manifest) {
        let trimmed = msg
            .strip_prefix("Toolchain mismatch: ")
            .unwrap_or(&msg)
            .to_string();
        error!("toolchain mismatch", trimmed);
        return 1;
    }

    if let Err(msg) = deps::validate_project_name(&manifest.project.name) {
        cli_error!(
            "Invalid project name",
            msg,
            "Rename `project.name` in `lisette.toml`"
        );
        return 1;
    }

    print_preview_notice("lis sync");

    let target_dir = project_root.join("target");
    if target_dir.is_file() {
        cli_error!(
            "Failed to set up target directory",
            "`target/` exists but is a file, not a directory",
            "Remove or move `target/` and retry"
        );
        return 1;
    }
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        error!(
            "failed to set up target directory",
            format!("Failed to create target directory: {}", e)
        );
        return 1;
    }

    let _lock = match acquire_mutation_lock(&target_dir) {
        Ok(f) => f,
        Err(code) => return code,
    };

    let imported_pkgs = match scan_source_imports(&project_root.join("src")) {
        Ok(pkgs) => pkgs,
        Err(SourceScanError::Parse { path, message }) => {
            cli_error!(
                "Source parse error",
                format!("Failed to parse `{}`: {}", path.display(), message),
                "Fix the parse error and rerun `lis sync`"
            );
            return 1;
        }
        Err(SourceScanError::Read { path, error }) => {
            error!(
                "failed to read source file",
                format!("Failed to read `{}`: {}", path.display(), error)
            );
            return 1;
        }
    };

    let trimmed = match deps::trim_dead_via_parents(&project_root) {
        Ok(t) => t,
        Err(msg) => {
            error!("failed to update manifest", msg);
            return 1;
        }
    };

    let report = match deps::resolve_empty_via(&project_root, &imported_pkgs) {
        Ok(r) => r,
        Err(msg) => {
            error!("failed to update manifest", msg);
            return 1;
        }
    };

    print_sync_summary(&trimmed, &report.promoted, &report.removed);

    0
}

enum SourceScanError {
    Parse {
        path: PathBuf,
        message: String,
    },
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
}

/// Collect every third-party Go package path imported via `import "go:..."`
/// across `src/**/*.lis`. Aborts on the first parse or read error.
fn scan_source_imports(src_dir: &Path) -> Result<Vec<String>, SourceScanError> {
    let mut imports = Vec::new();
    if !src_dir.is_dir() {
        return Ok(imports);
    }

    for path in collect_lis_filepaths_recursive(src_dir) {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return Err(SourceScanError::Read { path, error: e }),
        };
        let parse_result = Parser::lex_and_parse_file(&source, 0);
        if parse_result.failed() {
            return Err(SourceScanError::Parse {
                path,
                message: parse_result.errors[0].message.clone(),
            });
        }
        for expr in &parse_result.ast {
            if let Expression::ModuleImport { name, .. } = expr
                && let Some(pkg) = name.strip_prefix("go:")
                && deps::is_third_party(pkg)
            {
                imports.push(pkg.to_string());
            }
        }
    }

    Ok(imports)
}
