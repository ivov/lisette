use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use deps::TypedefLocator;
use diagnostics::render::{self, Filter};
use lisette::fs::LocalFileSystem;
use lisette::pipeline::{CompileConfig, CompilePhase, CompileResult, compile};

use crate::cli_error;
use crate::typedef_regen::generate_missing_typedefs;

pub fn check(path: Option<String>, errors_only: bool, warnings_only: bool) -> i32 {
    let target = path.unwrap_or_else(|| ".".to_string());
    let target_path = Path::new(&target);

    if !target_path.exists() {
        cli_error!(
            "Failed to check",
            format!("Path `{}` does not exist", target),
            "Check the path and try again"
        );
        return 1;
    }

    let filter = Filter {
        errors_only,
        warnings_only,
    };

    if !target_path.is_dir() {
        return check_single_file(target_path, &filter, false, TypedefLocator::default());
    }

    if target_path.join("lisette.toml").exists() {
        return check_project(target_path, &filter);
    }

    check_loose_dir(target_path, &filter)
}

fn check_project(project_path: &Path, filter: &Filter) -> i32 {
    let root_main = project_path.join("main.lis");
    let src_main = project_path.join("src/main.lis");

    if root_main.exists() {
        cli_error!(
            "Misplaced entrypoint",
            "Found `main.lis` in project root, expected it at `src/main.lis`",
            "Move `main.lis` to `src/main.lis`"
        );
        return 1;
    }

    if !src_main.exists() {
        cli_error!(
            "Failed to lint and typecheck project",
            format!("No `src/main.lis` at `{}`", project_path.display()),
            "Create `src/main.lis`"
        );
        return 1;
    }

    let (manifest, locator) = match deps::TypedefLocator::from_project_with_manifest(project_path) {
        Ok(pair) => pair,
        Err(msg) => {
            cli_error!("Failed to check project", msg, "Fix `lisette.toml`");
            return 1;
        }
    };

    if let Err(code) = generate_missing_typedefs(project_path, &manifest) {
        return code;
    }

    check_single_file(&src_main, filter, true, locator)
}

fn check_single_file(
    file_path: &Path,
    filter: &Filter,
    load_siblings: bool,
    locator: TypedefLocator,
) -> i32 {
    let start = Instant::now();
    let Some((result, source, filename)) = compile_single_file(file_path, load_siblings, locator)
    else {
        return 1; // Read error already reported by compile_single_file
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
        filter,
        &source,
        &filename,
    );
    render::print_summary(
        counts.files,
        start.elapsed(),
        counts.errors,
        counts.warnings,
    );
    counts.errors
}

fn compile_single_file(
    file_path: &Path,
    load_siblings: bool,
    locator: TypedefLocator,
) -> Option<(CompileResult, String, String)> {
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            cli_error!(
                "Failed to check",
                format!("Failed to read `{}`: {}", file_path.display(), e),
                "Check file permissions"
            );
            return None;
        }
    };

    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("main.lis");

    let config = CompileConfig {
        target_phase: CompilePhase::Check,
        go_module: "main".to_string(),
        standalone_mode: !load_siblings,
        load_siblings,
        debug: false,
        project_root: locator.project_root().map(|p| p.to_path_buf()),
        locator,
    };

    let working_dir = file_path.parent().and_then(|p| p.to_str()).unwrap_or(".");

    let fs = LocalFileSystem::new(working_dir);
    let result = compile(&source, filename, &config, &fs);
    let display_filename = file_path.display().to_string();

    Some((result, source, display_filename))
}

fn check_loose_dir(dir: &Path, filter: &Filter) -> i32 {
    let mut files = lisette::fs::collect_lis_filepaths_recursive(dir);
    files.sort();

    if files.is_empty() {
        cli_error!(
            "Failed to check",
            format!("No `.lis` files found in `{}`", dir.display()),
            "Provide a path to a `.lis` file or directory containing `.lis` files"
        );
        return 1;
    }

    let mut dirs: HashMap<PathBuf, Vec<PathBuf>> = HashMap::default();
    for file_path in &files {
        if let Some(parent) = file_path.parent() {
            dirs.entry(parent.to_path_buf())
                .or_default()
                .push(file_path.clone());
        }
    }

    let mut total_errors = 0;
    let mut total_warnings = 0;
    let mut total_files = 0;
    let mut read_failures = 0;

    let start = Instant::now();

    for dir_files in dirs.values() {
        let mut compiled = None;
        let mut dir_read_failures = 0;
        for file in dir_files {
            if let Some(result) = compile_single_file(file, true, TypedefLocator::default()) {
                compiled = Some(result);
                break;
            }
            dir_read_failures += 1;
        }

        let Some((result, source, filename)) = compiled else {
            read_failures += dir_read_failures;
            continue;
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
            filter,
            &source,
            &filename,
        );
        total_errors += counts.errors;
        total_warnings += counts.warnings;
        total_files += result.user_file_count;
    }

    let elapsed = start.elapsed();

    let all_errors = total_errors + read_failures;
    render::print_summary(total_files, elapsed, all_errors, total_warnings);

    all_errors
}
