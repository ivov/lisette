use std::path::{Path, PathBuf};
use std::sync::Arc;

use stdlib::Target;
use syntax::ast::{Expression, ImportAlias};
use syntax::parse::Parser;

use lisette::fs::collect_lis_filepaths_recursive;

use crate::go_cli;
use crate::handlers::add::find_project_root;
use crate::lock::{acquire_mutation_lock, acquire_target_lock};
use crate::output::{print_preview_notice, print_sync_summary};
use crate::typedef_regen::prewarm_typedef_cache;
use crate::workspace::WorkspaceBindgen;
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

    if let Err(msg) = deps::check_no_subpackage_deps(&manifest) {
        cli_error!(
            "Invalid `lisette.toml`",
            msg,
            "Fix `lisette.toml` and retry"
        );
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

    let _mutation_lock = match acquire_mutation_lock(&target_dir) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let _target_lock = match acquire_target_lock(&target_dir) {
        Ok(f) => f,
        Err(code) => return code,
    };

    let scanned = match scan_source_imports(&project_root.join("src")) {
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

    let mut bindgen_runner: Option<Arc<WorkspaceBindgen>> = None;
    let prewarm_result = if !scanned.non_blank.is_empty() {
        let target = Target::host();

        let locator =
            deps::TypedefLocator::new(manifest.go_deps(), Some(project_root.clone()), target);
        if let Err(msg) = go_cli::write_go_mod(&target_dir, &manifest.project.name, &locator) {
            error!("failed to write target/go.mod", msg);
            return 1;
        }

        let typedef_cache_dir = deps::typedef_cache_dir(&project_root);
        let runner = Arc::new(WorkspaceBindgen::new(
            target_dir.clone(),
            typedef_cache_dir,
            target,
        ));
        let locator = locator.with_bindgen(runner.clone());
        bindgen_runner = Some(runner);

        prewarm_typedef_cache(&scanned.non_blank, &locator)
    } else {
        Ok(())
    };

    let trimmed = match deps::trim_dead_via_parents(&project_root) {
        Ok(t) => t,
        Err(msg) => {
            error!("failed to update manifest", msg);
            return 1;
        }
    };

    let report = match deps::resolve_empty_via(&project_root, &scanned.all) {
        Ok(r) => r,
        Err(msg) => {
            error!("failed to update manifest", msg);
            return 1;
        }
    };

    let needs_separator = bindgen_runner
        .as_ref()
        .is_some_and(|r| r.progress_emitted());
    print_sync_summary(&trimmed, &report.promoted, &report.removed, needs_separator);

    prewarm_result.err().unwrap_or(0)
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

struct ScannedImports {
    /// All third-party `go:` imports (blank-imports keep modules referenced).
    all: Vec<String>,
    /// Third-party `go:` imports excluding `_`-aliased blank ones.
    non_blank: Vec<String>,
}

/// Collect every third-party `go:` import across `src/**/*.lis`.
fn scan_source_imports(src_dir: &Path) -> Result<ScannedImports, SourceScanError> {
    let mut all = Vec::new();
    let mut non_blank = Vec::new();
    if !src_dir.is_dir() {
        return Ok(ScannedImports { all, non_blank });
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
            if let Expression::ModuleImport { name, alias, .. } = expr
                && let Some(pkg) = name.strip_prefix("go:")
                && deps::is_third_party(pkg)
            {
                all.push(pkg.to_string());
                if !matches!(alias, Some(ImportAlias::Blank(_))) {
                    non_blank.push(pkg.to_string());
                }
            }
        }
    }

    Ok(ScannedImports { all, non_blank })
}
