use std::sync::Arc;

use stdlib::Target;

use crate::go_cli;
use crate::handlers::add::find_project_root;
use crate::handlers::reconciliation::{finalize_manifest_via, reconcile_declared_replacements};
use crate::lock::{acquire_mutation_lock, acquire_target_lock};
use crate::output::print_sync_summary;
use crate::typedef_regen::prewarm_typedef_cache;
use crate::typedef_scan::{SourceScanError, scan_source_imports};
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

    // Drop dead `via` entries (replaced ones included) before any go.mod write,
    // so a stale replacement cannot poison later Go commands.
    let (pre_trimmed, pre_report) = match finalize_manifest_via(&project_root, &scanned.all) {
        Ok(reports) => reports,
        Err(code) => return code,
    };
    let manifest = match deps::parse_manifest(&project_root) {
        Ok(m) => m,
        Err(msg) => {
            error!("failed to read manifest", msg);
            return 1;
        }
    };

    if let Err(code) = reconcile_declared_replacements(&project_root, &target_dir, &manifest) {
        return code;
    }

    let manifest = match deps::parse_manifest(&project_root) {
        Ok(m) => m,
        Err(msg) => {
            error!("failed to read manifest", msg);
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

    let (post_trimmed, post_report) = match finalize_manifest_via(&project_root, &scanned.all) {
        Ok(reports) => reports,
        Err(code) => return code,
    };

    let mut trimmed = pre_trimmed;
    trimmed.extend(post_trimmed);
    let mut promoted = pre_report.promoted;
    promoted.extend(post_report.promoted);
    let mut removed = pre_report.removed;
    removed.extend(post_report.removed);

    let needs_separator = bindgen_runner
        .as_ref()
        .is_some_and(|r| r.progress_emitted());
    print_sync_summary(&trimmed, &promoted, &removed, needs_separator);

    prewarm_result.err().unwrap_or(0)
}
