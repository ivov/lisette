use std::sync::Arc;

use stdlib::Target;

use crate::go_cli;
use crate::handlers::project::MutationProject;
use crate::handlers::reconciliation::{finalize_manifest_via, reconcile_declared_replacements};
use crate::output::print_sync_summary;
use crate::typedef_regen::prewarm_typedef_cache;
use crate::typedef_scan::{SourceScanError, scan_source_imports};
use crate::workspace::WorkspaceBindgen;
use crate::{cli_error, error};

pub fn sync(script: Option<&str>) -> i32 {
    if let Some(script) = script {
        return super::script_sync::run(std::path::Path::new(script));
    }

    let project = match MutationProject::open() {
        Ok(project) => project,
        Err(code) => return code,
    };
    let project_root = &project.root;
    let target_dir = &project.target_dir;

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
    let all_imports: Vec<String> = scanned.all().map(str::to_string).collect();
    let non_blank_imports: Vec<String> = scanned.non_blank().map(str::to_string).collect();

    // Drop dead `via` entries (replaced ones included) before any go.mod write,
    // so a stale replacement cannot poison later Go commands.
    let mut changes = match finalize_manifest_via(project_root, &all_imports) {
        Ok(changes) => changes,
        Err(code) => return code,
    };
    let manifest = match deps::parse_manifest(project_root) {
        Ok(m) => m,
        Err(msg) => {
            error!("failed to read manifest", msg);
            return 1;
        }
    };

    if let Err(code) = reconcile_declared_replacements(project_root, target_dir, &manifest) {
        return code;
    }

    let manifest = match deps::parse_manifest(project_root) {
        Ok(m) => m,
        Err(msg) => {
            error!("failed to read manifest", msg);
            return 1;
        }
    };

    let (prewarm_result, needs_separator) = if !non_blank_imports.is_empty() {
        let target = Target::host();

        let locator =
            deps::TypedefLocator::new(manifest.go_deps(), Some(project_root.clone()), target);
        if let Err(msg) = go_cli::write_go_mod(target_dir, &manifest.project.name, &locator) {
            error!("failed to write target/go.mod", msg);
            return 1;
        }

        let typedef_cache_dir = &project.typedef_cache_dir;
        let runner = Arc::new(WorkspaceBindgen::new(
            target_dir.clone(),
            typedef_cache_dir.clone(),
            target,
        ));
        let locator = locator.with_bindgen(runner.clone());
        let result = prewarm_typedef_cache(&non_blank_imports, &locator);
        (result, runner.progress_emitted())
    } else {
        (Ok(()), false)
    };

    let post_changes = match finalize_manifest_via(project_root, &all_imports) {
        Ok(changes) => changes,
        Err(code) => return code,
    };
    changes.extend(post_changes);

    print_sync_summary(
        "Manifest",
        &changes.trimmed,
        &changes.promoted,
        &changes.removed,
        needs_separator,
    );

    prewarm_result.err().unwrap_or(0)
}
