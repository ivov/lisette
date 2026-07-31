use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use crate::cli_error;
use crate::go_cli;
use diagnostics::render::{self, Filter};
use lisette::pipeline::{
    CompileConfig, CompileEntry, CompileInput, CompileMode, CompileScope, ProjectKind, compile,
};
use semantics::loader::MemoryLoader;

fn exec_binary(output_path: &Path, args: &[String], heading: &str) -> i32 {
    match Command::new(output_path).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to execute compiled binary: {}", e),
                "Check that the binary was produced and is executable"
            );
            1
        }
    }
}

pub fn run(
    target: Option<String>,
    args: Vec<String>,
    sourcemap: bool,
    go_flags: Vec<String>,
) -> i32 {
    let target = target.unwrap_or_else(|| ".".to_string());

    if target.ends_with(".lis") {
        run_standalone(&target, args, sourcemap, &go_flags)
    } else {
        run_project(&target, args, sourcemap, &go_flags)
    }
}

fn run_project(path: &str, args: Vec<String>, sourcemap: bool, go_flags: &[String]) -> i32 {
    let project_path = Path::new(path);

    let project = match super::build::LockedProject::acquire(project_path) {
        Ok(project) => project,
        Err(code) => return code,
    };

    if project.kind == ProjectKind::Library {
        cli_error!(
            "Nothing to run",
            format!(
                "`{}` is a library, as it has no `src/main.lis` entrypoint",
                project.manifest.project.name
            ),
            "If not meant to be a library, convert it to a binary by adding `src/main.lis`"
        );
        return 1;
    }

    let heading = "Failed to run project";
    let target = stdlib::Target::host();

    if let Err(code) =
        super::build::build_locked(&project, super::build::BuildPurpose::Run { sourcemap })
    {
        return code;
    }

    let output_path = match super::build::link_project_binary(&project, go_flags, target, heading) {
        Ok(p) => p,
        Err(code) => return code,
    };

    exec_binary(&output_path, &args, heading)
}

fn run_standalone(file: &str, args: Vec<String>, sourcemap: bool, go_flags: &[String]) -> i32 {
    let file_path = Path::new(file);

    if !file_path.exists() {
        cli_error!(
            "Failed to run standalone file",
            format!("File `{}` does not exist", file),
            "Check the file path and try again"
        );
        return 1;
    }

    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            cli_error!(
                "Failed to run standalone file",
                format!("Failed to read `{}`: {}", file, e),
                "Check file permissions"
            );
            return 1;
        }
    };

    let absolute_path = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    absolute_path.hash(&mut hasher);
    let hash = hasher.finish();
    let temp_dir = std::env::temp_dir().join(format!("lis-run-{:x}", hash));

    if let Err(e) = fs::create_dir_all(&temp_dir) {
        cli_error!(
            "Failed to run standalone file",
            format!("Failed to create temporary directory: {}", e),
            "Check permissions on temp directory"
        );
        return 1;
    }

    // Absolute path required: a relative `TMPDIR` would break the `-o`/exec contract.
    let temp_dir = match temp_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            cli_error!(
                "Failed to run standalone file",
                format!("Failed to resolve temporary directory: {}", e),
                "Check permissions on temp directory"
            );
            return 1;
        }
    };

    let locator = deps::TypedefLocator::default();
    let compile_config = CompileConfig {
        mode: CompileMode::Emit { sourcemap },
        go_module: "lis-standalone",
        entry_package_name: "main",
        scope: CompileScope::Standalone,
        locator: &locator,
    };

    let entry_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file)
        .to_string();
    let entry_display = lisette::fs::relative_to_cwd(file_path).unwrap_or_else(|| file.to_string());

    let no_loader = MemoryLoader::new();
    let result = compile(
        CompileInput::Binary(CompileEntry {
            source: &source,
            filename: &entry_name,
            display_path: &entry_display,
        }),
        compile_config,
        &no_loader,
    );

    let filter = Filter::All;

    let counts = render::render_all(
        &result.diagnostics,
        render::SourceCache::new(
            |file_id| {
                result
                    .sources
                    .get(&file_id)
                    .map(|info| (info.source.clone(), info.filename.clone()))
            },
            &source,
            &entry_display,
        ),
        result.user_file_count,
        &filter,
    );

    if counts.errors > 0 {
        return 1;
    }

    if let Err(e) = go_cli::write_go_mod(&temp_dir, "lis-standalone", &locator) {
        cli_error!("Failed to run standalone file", e, "Check file permissions");
        return 1;
    }

    let heading = "Failed to run standalone file";

    let emit = match go_cli::write_go_outputs(&temp_dir, &result.output) {
        Ok(emit) => emit,
        Err(e) => {
            cli_error!(heading, e.message, e.hint);
            return 1;
        }
    };

    let target = locator.target();
    let import_set_hash = go_cli::compute_import_set_hash(&emit.new_manifest, "lis-standalone");

    if let Err(e) = go_cli::finalize_go_dir(&temp_dir, target, &emit.changed, import_set_hash) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }

    go_cli::write_emit_manifest(&temp_dir, &emit.new_manifest);

    let output_path = temp_dir.join(go_cli::run_binary_name(target));
    if let Err(e) = go_cli::build_binary(&temp_dir, &output_path, target, go_flags) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }
    exec_binary(&output_path, &args, heading)
}
