use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::cli_error;
use crate::go_cli;
use crate::typedef_regen::generate_missing_typedefs;
use diagnostics::render::{self, Filter};
use lisette::fs::{LocalFileSystem, prune_orphan_go_files};
use lisette::pipeline::{CompileConfig, CompilePhase, compile};

pub fn build(path: Option<String>, debug: bool, quiet: bool) -> i32 {
    if let Err(code) = crate::go_cli::require_go() {
        return code;
    }

    let start = Instant::now();

    let project_root = path.unwrap_or_else(|| ".".to_string());
    let project_path = Path::new(&project_root);

    if !validate_project(project_path) {
        return 1;
    }

    let (manifest, locator) = match deps::TypedefLocator::from_project_with_manifest(project_path) {
        Ok(pair) => pair,
        Err(msg) => {
            cli_error!(
                "Failed to compile Lisette project to Go",
                msg,
                "Run `lis new <name>` to create a project, or fix `lisette.toml`"
            );
            return 1;
        }
    };

    if let Err(code) = generate_missing_typedefs(project_path, &manifest) {
        return code;
    }

    let main_lis = project_path.join("src/main.lis");
    let go_module_name = &manifest.project.name;
    let version = &manifest.project.version;

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

    let display_path = std::env::current_dir()
        .ok()
        .and_then(|cwd| main_lis.strip_prefix(&cwd).ok().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| main_lis.to_path_buf());

    let filename = "main.lis";

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
        project_root: Some(project_path.to_path_buf()),
        locator: locator.clone(),
    };

    let source_dir = main_lis.parent().and_then(|p| p.to_str()).unwrap_or(".");
    let local_fs = LocalFileSystem::new(source_dir);

    let result = compile(&main_lis_source, filename, &compile_config, &local_fs);

    let filename = display_path.display().to_string();
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
        &filename,
    );

    if counts.errors > 0 {
        return 1;
    }

    let target_dir = project_path.join("target");
    if let Err(e) = fs::create_dir_all(&target_dir) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Failed to create `target` directory: {}", e),
            "Check directory permissions"
        );
        return 1;
    }

    if let Err(e) = go_cli::write_go_mod(&target_dir, &compile_config.go_module, &locator) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            e,
            "Check file permissions"
        );
        return 1;
    }

    for file in &result.output {
        let go_file_path = target_dir.join(&file.name);
        let go_code = file.to_go();

        if let Some(parent) = go_file_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            cli_error!(
                "Failed to compile Lisette project to Go",
                format!("Failed to create directory `{}`: {}", parent.display(), e),
                "Check directory permissions"
            );
            return 1;
        }

        if let Err(e) = fs::write(&go_file_path, &go_code) {
            cli_error!(
                "Failed to compile Lisette project to Go",
                format!("Failed to write `{}`: {}", go_file_path.display(), e),
                "Check file permissions"
            );
            return 1;
        }
    }

    let produced: Vec<&str> = result
        .output
        .iter()
        .map(|file| file.name.as_str())
        .collect();
    if let Err(e) = prune_orphan_go_files(&target_dir, &produced) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Failed to prune stale Go files: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    if let Err(e) = go_cli::go_fmt(&target_dir) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Go format failed: {}", e),
            "Check Go installation with `go version`"
        );
        return 1;
    }

    if let Err(e) = go_cli::ensure_go_sum(&target_dir) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Failed to resolve Go dependencies: {}", e),
            "Check Go installation and network connectivity"
        );
        return 1;
    }

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
            "Failed to compile Lisette project to Go",
            format!("Path `{}` does not exist", project_path.display()),
            "Check the path and try again"
        );
        return false;
    }

    if project_path.is_file() {
        cli_error!(
            "Failed to compile Lisette project to Go",
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
