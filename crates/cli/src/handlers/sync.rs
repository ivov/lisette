use std::sync::Arc;

use stdlib::Target;

use crate::go_cli;
use crate::handlers::project::MutationProject;
use crate::handlers::reconciliation::{
    declare_reached_modules, finalize_manifest_via, reconcile_declared_replacements,
};
use crate::output::print_sync_summary;
use crate::typedef_regen::{PrewarmOutcome, prewarm_typedef_cache};
use crate::typedef_scan::{SourceScanError, scan_source_imports};
use crate::workspace::{GoWorkspace, WorkspaceBindgen, warm_typedefs};
use crate::{cli_error, error};
use std::path::Path;

pub fn sync(script: Option<&str>) -> i32 {
    if let Some(script) = script {
        return super::script_sync::run(Path::new(script));
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

    let mut added = Vec::new();
    let (prewarm_result, needs_separator) = if !non_blank_imports.is_empty() {
        match prewarm_declaring_missing(&project, &non_blank_imports, &mut added) {
            Ok(prewarmed) => prewarmed,
            Err(code) => return code,
        }
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
        &added,
        &changes.trimmed,
        &changes.promoted,
        &changes.removed,
        needs_separator,
    );

    prewarm_result.err().unwrap_or(0)
}

fn read_manifest(project_root: &Path) -> Result<deps::Manifest, i32> {
    deps::parse_manifest(project_root).map_err(|msg| {
        error!("failed to read manifest", msg);
        1
    })
}

/// Prewarm, declaring missing modules into `added` until none are missing.
fn prewarm_declaring_missing(
    project: &MutationProject,
    imports: &[String],
    added: &mut Vec<String>,
) -> Result<(Result<(), i32>, bool), i32> {
    let (mut outcome, mut printed) = prewarm(project, imports)?;
    while !outcome.missing_transitive.is_empty() {
        let manifest = read_manifest(&project.root)?;
        let declared =
            declare_reached_modules(&project.root, &project.target_dir, &manifest, imports)?;
        if declared.is_empty() {
            let packages: Vec<String> = outcome
                .missing_transitive
                .iter()
                .map(|pkg| format!("`{}`", pkg))
                .collect();
            cli_error!(
                "Undeclared Go dependency",
                format!(
                    "No declared module contains {}, which Go typedefs import",
                    packages.join(", ")
                ),
                "Declare the module that contains each package with `lis add <module>`"
            );
            return Err(1);
        }
        added.extend(declared);
        let (next, next_printed) = prewarm(project, imports)?;
        outcome = next;
        printed |= next_printed;
    }
    Ok((outcome.result, printed))
}

/// Write `target/go.mod`, then prewarm. The bool tells if progress was printed.
fn prewarm(project: &MutationProject, imports: &[String]) -> Result<(PrewarmOutcome, bool), i32> {
    let project_root = &project.root;
    let target_dir = &project.target_dir;
    let manifest = read_manifest(project_root)?;
    let target = Target::host();

    let locator = deps::TypedefLocator::new(
        manifest.go_deps().clone(),
        Some(project_root.clone()),
        target,
    );
    let go_directive = go_cli::project_go_directive(project_root);
    if let Err(msg) =
        go_cli::write_go_mod(target_dir, &manifest.project.name, &locator, &go_directive)
    {
        error!("failed to write target/go.mod", msg);
        return Err(1);
    }

    let typedef_cache_dir = &project.typedef_cache_dir;
    let runner = Arc::new(WorkspaceBindgen::new(
        target_dir.clone(),
        typedef_cache_dir.clone(),
        target,
    ));
    let locator = locator.with_bindgen(runner.clone());
    let workspace = GoWorkspace::new(target_dir, typedef_cache_dir, target);
    let warmed = warm_typedefs(project_root, &workspace, &locator);
    let outcome = prewarm_typedef_cache(imports, &locator);
    Ok((outcome, warmed || runner.progress_emitted()))
}
