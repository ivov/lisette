use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::cli_error;
use crate::go_cli;
use crate::lock::acquire_target_lock;
use crate::workspace::WorkspaceBindgen;
use diagnostics::render::{self, Filter};
use lisette::fs::{LocalFileSystem, prune_orphan_go_files};
use lisette::pipeline::{CompileConfig, CompilePhase, compile};

pub fn build(path: Option<String>, debug: bool, quiet: bool) -> i32 {
    let project_root = path.unwrap_or_else(|| ".".to_string());
    let project_path = Path::new(&project_root);

    let prep = match prepare_project_build(project_path) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let _target_lock = match acquire_target_lock(&prep.target_dir) {
        Ok(f) => f,
        Err(code) => return code,
    };

    build_locked(&prep, debug, quiet)
}

pub(super) fn prepare_project_build(project_path: &Path) -> Result<BuildPrep, i32> {
    crate::go_cli::require_go()?;

    if !validate_project(project_path) {
        return Err(1);
    }

    let (manifest, locator) = match deps::TypedefLocator::from_project_with_manifest(project_path) {
        Ok(pair) => pair,
        Err(msg) => {
            cli_error!(
                "Failed to compile Lisette project to Go",
                msg,
                "Run `lis new <name>` to create a project, or fix `lisette.toml`"
            );
            return Err(1);
        }
    };

    let target_dir = project_path.join("target");
    if let Err(e) = fs::create_dir_all(&target_dir) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Failed to create `target` directory: {}", e),
            "Check directory permissions"
        );
        return Err(1);
    }

    Ok(BuildPrep {
        project_path: project_path.to_path_buf(),
        target_dir,
        manifest,
        locator,
    })
}

pub(super) struct BuildPrep {
    pub project_path: PathBuf,
    pub target_dir: PathBuf,
    pub manifest: deps::Manifest,
    pub locator: deps::TypedefLocator,
}

pub(super) fn build_locked(prep: &BuildPrep, debug: bool, quiet: bool) -> i32 {
    let start = Instant::now();

    if let Err(e) =
        go_cli::write_go_mod(&prep.target_dir, &prep.manifest.project.name, &prep.locator)
    {
        cli_error!(
            "Failed to compile Lisette project to Go",
            e,
            "Check file permissions on `target/go.mod`"
        );
        return 1;
    }

    let typedef_cache_dir = deps::typedef_cache_dir(&prep.project_path);
    let bindgen = Arc::new(WorkspaceBindgen::new(
        prep.target_dir.clone(),
        typedef_cache_dir,
        prep.locator.target(),
    ));
    let locator = prep.locator.clone().with_bindgen(bindgen);

    let main_lis = prep.project_path.join("src/main.lis");
    let go_module_name = &prep.manifest.project.name;
    let version = &prep.manifest.project.version;

    let main_lis_source = match fs::read_to_string(&main_lis) {
        Ok(s) => s,
        Err(e) => {
            cli_error!(
                "Failed to compile Lisette project to Go",
                format!("Failed to read `{}`: {}", main_lis.display(), e),
                "Check file permissions"
            );
            return 1;
        }
    };

    let entry_name = main_lis
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("main.lis")
        .to_string();
    let entry_display =
        lisette::fs::relative_to_cwd(&main_lis).unwrap_or_else(|| entry_name.clone());

    let project_name = go_module_name.rsplit('/').next().unwrap_or(go_module_name);

    if !quiet {
        eprintln!();
        if crate::output::use_color() {
            use owo_colors::OwoColorize;
            eprintln!(
                "  · Compiling {} v{}",
                project_name.bright_magenta(),
                version
            );
        } else {
            eprintln!("  · Compiling `{}` v{}", project_name, version);
        }
    }

    let compile_config = CompileConfig {
        target_phase: CompilePhase::Emit,
        go_module: go_module_name.to_string(),
        standalone_mode: false,
        load_siblings: true,
        debug,
        project_root: Some(prep.project_path.clone()),
        locator: locator.clone(),
    };

    let source_dir = main_lis.parent().and_then(|p| p.to_str()).unwrap_or(".");
    let local_fs = LocalFileSystem::new(source_dir);

    let result = compile(
        &main_lis_source,
        &entry_name,
        &entry_display,
        &compile_config,
        &local_fs,
    );

    let filter = Filter {
        errors_only: false,
        warnings_only: false,
    };

    let counts = render::render_all(
        &result.errors,
        &result.lints,
        |file_id| {
            result
                .sources
                .get(&file_id)
                .map(|info| (info.source.clone(), info.filename.clone()))
        },
        result.user_file_count,
        &filter,
        &main_lis_source,
        &entry_display,
    );

    if counts.errors > 0 {
        return 1;
    }

    let heading = "Failed to compile Lisette project to Go";

    if debug
        && let Err(e) = semantics::cache::apply_emit_stamps(
            &prep.project_path,
            &result
                .emit_stamps
                .iter()
                .map(|s| (s.clone(), None))
                .collect::<Vec<_>>(),
        )
    {
        cli_error!(
            heading,
            format!("Failed to invalidate emit stamps before debug write: {e}"),
            "Check file permissions on `target/cache`, or delete the directory and retry"
        );
        return 1;
    }

    let emit = match go_cli::write_go_outputs(&prep.target_dir, &result.output) {
        Ok(emit) => emit,
        Err(e) => {
            cli_error!(heading, e.message, e.hint);
            return 1;
        }
    };

    // Force a maximal go.mod rewrite only if a prior tidy marker exists and the
    // external import set changed since it was written.
    let import_set_hash =
        go_cli::compute_import_set_hash(&emit.new_manifest, &prep.manifest.project.name);
    if let Some(prior) = go_cli::read_tidy_marker(&prep.target_dir)
        && prior != import_set_hash
    {
        go_cli::invalidate_go_mod_stamp(&prep.target_dir);
        if let Err(e) =
            go_cli::write_go_mod(&prep.target_dir, &prep.manifest.project.name, &locator)
        {
            cli_error!(heading, e, "Check file permissions on `target/go.mod`");
            return 1;
        }
    }

    let produced: Vec<&str> = result
        .output
        .iter()
        .map(|file| file.name.as_str())
        .collect();
    if let Err(e) = prune_orphan_go_files(&prep.target_dir, &produced) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Failed to prune stale Go files: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    if let Err(e) = go_cli::finalize_go_dir(
        &prep.target_dir,
        locator.target(),
        &emit.changed,
        import_set_hash,
    ) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }

    if !debug
        && let Err(e) = semantics::cache::apply_emit_stamps(
            &prep.project_path,
            &result
                .emit_stamps
                .iter()
                .map(|s| (s.clone(), Some(s.artifact_hash)))
                .collect::<Vec<_>>(),
        )
    {
        eprintln!("warning: failed to write emit stamps: {e}");
    }

    // Committed only after gofmt + tidy succeed.
    go_cli::write_emit_manifest(&prep.target_dir, &emit.new_manifest);

    if !quiet {
        eprintln!(
            "  ✓ Build completed {}",
            crate::output::format_elapsed(start.elapsed())
        );
    }

    0
}

fn validate_project(project_path: &Path) -> bool {
    if !project_path.exists() {
        cli_error!(
            "Project not found",
            format!("Path `{}` does not exist", project_path.display()),
            "Check the path and try again"
        );
        return false;
    }

    if project_path.is_file() {
        cli_error!(
            "Not a project directory",
            format!(
                "Path `{}` is a file, not a project directory",
                project_path.display()
            ),
            "`lis build <path/to/dir>` to build a project, or use `lis run <path/to/file>` to run a single file standalone"
        );
        return false;
    }

    let root_main = project_path.join("main.lis");
    if root_main.exists() {
        cli_error!(
            "Misplaced entrypoint",
            "Found `main.lis` in project root, expected it at `src/main.lis`",
            "Move `main.lis` to `src/main.lis`"
        );
        return false;
    }

    let entrypoint = project_path.join("src/main.lis");
    if !entrypoint.exists() {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!(
                "No `src/main.lis` entrypoint in `{}`",
                project_path.display()
            ),
            "Create `src/main.lis`"
        );
        return false;
    }

    true
}
