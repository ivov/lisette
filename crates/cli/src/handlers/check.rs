use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use deps::TypedefLocator;
use diagnostics::render::{self, Filter, OutputFormat};
use diagnostics::{Fix, apply_fixes};
use lisette::fs::LocalFileSystem;
use lisette::pipeline::{CompileConfig, CompilePhase, CompileResult, compile};

use crate::cli_error;
use crate::lock::acquire_target_lock;
use crate::workspace::{GoWorkspace, WorkspaceBindgen, warm_typedefs};

pub fn check(
    path: Option<String>,
    errors_only: bool,
    warnings_only: bool,
    format: OutputFormat,
    fix: bool,
) -> i32 {
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
        return check_single_file(
            target_path,
            &filter,
            false,
            TypedefLocator::default(),
            format,
            fix,
        );
    }

    if target_path.join("lisette.toml").exists() {
        return check_project(target_path, &filter, format, fix);
    }

    check_loose_dir(target_path, &filter, format, fix)
}

fn check_project(project_path: &Path, filter: &Filter, format: OutputFormat, fix: bool) -> i32 {
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

    let target_dir = project_path.join("target");
    if let Err(e) = fs::create_dir_all(&target_dir) {
        cli_error!(
            "Failed to check project",
            format!("Failed to create target directory: {}", e),
            "Check directory permissions"
        );
        return 1;
    }

    let target_lock = match acquire_target_lock(&target_dir) {
        Ok(f) => f,
        Err(code) => return code,
    };

    if let Err(e) = crate::go_cli::write_go_mod(&target_dir, &manifest.project.name, &locator) {
        cli_error!(
            "Failed to check project",
            e,
            "Check file permissions on `target/go.mod`"
        );
        return 1;
    }

    let typedef_cache_dir = deps::typedef_cache_dir(project_path);

    // Batch-warm the typedef cache so the lazy path during compile is all hits.
    {
        let workspace = GoWorkspace::new(&target_dir, &typedef_cache_dir, locator.target());
        warm_typedefs(project_path, &workspace, &locator);
    }

    let bindgen = Arc::new(WorkspaceBindgen::new(
        target_dir,
        typedef_cache_dir,
        locator.target(),
    ));
    let locator = locator.with_bindgen(bindgen);

    let result = check_single_file(&src_main, filter, true, locator, format, fix);
    drop(target_lock);
    result
}

fn check_single_file(
    file_path: &Path,
    filter: &Filter,
    load_siblings: bool,
    locator: TypedefLocator,
    format: OutputFormat,
    fix: bool,
) -> i32 {
    let start = Instant::now();
    let unix = matches!(format, OutputFormat::Unix);
    if !unix {
        eprintln!();
    }
    let Some((result, source, filename)) = compile_single_file(file_path, load_siblings, locator)
    else {
        return 1; // Read error already reported by compile_single_file
    };

    if fix {
        let mut summary = FixSummary::default();
        apply_result_fixes(&result, &mut summary);
        print_fix_summary(&summary, start.elapsed());
        return i32::from(summary.write_failures > 0);
    }

    let get_source = |file_id: u32| {
        result
            .sources
            .get(&file_id)
            .map(|info| (info.source.clone(), info.filename.clone()))
    };
    let counts = if unix {
        let (output, counts) = render::render_unix(
            &result.errors,
            &result.lints,
            get_source,
            result.user_file_count,
            filter,
            &source,
            &filename,
        );
        print!("{}", output);
        counts
    } else {
        render::render_all(
            &result.errors,
            &result.lints,
            get_source,
            result.user_file_count,
            filter,
            &source,
            &filename,
        )
    };
    if !unix {
        render::print_summary(
            counts.files,
            start.elapsed(),
            counts.errors,
            counts.warnings,
            counts.info,
        );
    }
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

    let entry_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("main.lis")
        .to_string();
    let entry_display =
        lisette::fs::relative_to_cwd(file_path).unwrap_or_else(|| entry_name.clone());

    let config = CompileConfig {
        target_phase: CompilePhase::Check,
        go_module: "main".to_string(),
        standalone_mode: !load_siblings,
        load_siblings,
        sourcemap: false,
        emit_tests: false,
        project_root: locator.project_root().map(|p| p.to_path_buf()),
        locator,
    };

    let working_dir = file_path.parent().and_then(|p| p.to_str()).unwrap_or(".");

    let fs = LocalFileSystem::new(working_dir);
    let result = compile(&source, &entry_name, &entry_display, &config, &fs);

    Some((result, source, entry_display))
}

fn check_loose_dir(dir: &Path, filter: &Filter, format: OutputFormat, fix: bool) -> i32 {
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
    let mut total_info = 0;
    let mut total_files = 0;
    let mut read_failures = 0;

    let unix = matches!(format, OutputFormat::Unix);
    let start = Instant::now();
    if !unix {
        eprintln!();
    }

    let mut fix_summary = FixSummary::default();

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

        if fix {
            apply_result_fixes(&result, &mut fix_summary);
            continue;
        }

        let get_source = |file_id: u32| {
            result
                .sources
                .get(&file_id)
                .map(|info| (info.source.clone(), info.filename.clone()))
        };
        let counts = if unix {
            let (output, counts) = render::render_unix(
                &result.errors,
                &result.lints,
                get_source,
                result.user_file_count,
                filter,
                &source,
                &filename,
            );
            print!("{}", output);
            counts
        } else {
            render::render_all(
                &result.errors,
                &result.lints,
                get_source,
                result.user_file_count,
                filter,
                &source,
                &filename,
            )
        };
        total_errors += counts.errors;
        total_warnings += counts.warnings;
        total_info += counts.info;
        total_files += result.user_file_count;
    }

    let elapsed = start.elapsed();

    if fix {
        print_fix_summary(&fix_summary, elapsed);
        return i32::from(fix_summary.write_failures > 0);
    }

    let all_errors = total_errors + read_failures;
    if !unix {
        render::print_summary(total_files, elapsed, all_errors, total_warnings, total_info);
    }

    all_errors
}

#[derive(Default)]
struct FixSummary {
    applied: usize,
    files_changed: usize,
    write_failures: usize,
}

fn apply_result_fixes(result: &CompileResult, summary: &mut FixSummary) {
    let mut by_file: HashMap<u32, Vec<&Fix>> = HashMap::default();
    for lint in &result.lints {
        let Some(fix) = lint.fix() else {
            continue;
        };
        let Some(file_id) = lint.file_id() else {
            continue;
        };
        by_file.entry(file_id).or_default().push(fix);
    }

    for (file_id, fixes) in by_file {
        let Some(info) = result.sources.get(&file_id) else {
            continue;
        };
        let path = Path::new(&info.filename);
        if !path.is_file() {
            continue;
        }

        let applied = apply_fixes(&info.source, fixes);
        if applied.applied == 0 {
            continue;
        }

        if !syntax::build_ast(&applied.source, file_id)
            .errors
            .is_empty()
        {
            cli_error!(
                "Skipped a fix",
                format!(
                    "Applying fixes to `{}` would produce invalid syntax",
                    info.filename
                ),
                "Re-run `lis check` to see the remaining diagnostics"
            );
            summary.write_failures += 1;
            continue;
        }

        match fs::File::create(path).and_then(|mut file| file.write_all(applied.source.as_bytes()))
        {
            Ok(()) => {
                summary.applied += applied.applied;
                summary.files_changed += 1;
            }
            Err(e) => {
                cli_error!(
                    "Failed to write fix",
                    format!("Failed to write `{}`: {}", info.filename, e),
                    "Check file permissions"
                );
                summary.write_failures += 1;
            }
        }
    }
}

fn print_fix_summary(summary: &FixSummary, elapsed: std::time::Duration) {
    let time_display = crate::output::format_elapsed(elapsed);

    if summary.files_changed == 0 {
        eprintln!("  ✓ No fixes applied {}", time_display);
    } else {
        let fix_word = if summary.applied == 1 { "fix" } else { "fixes" };
        let location = if summary.files_changed == 1 {
            "in 1 file".to_string()
        } else {
            format!("across {} files", summary.files_changed)
        };
        eprintln!(
            "  ✓ Applied {} {} {} {}",
            summary.applied, fix_word, location, time_display
        );
    }
}
