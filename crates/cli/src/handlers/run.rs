use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use crate::cli_error;
use crate::go_cli;
use diagnostics::render::{self, Filter};
use lisette::pipeline::{CompileConfig, CompilePhase, compile};
use semantics::loader::MemoryLoader;

fn build_and_exec(
    build_dir: &Path,
    output_path: &Path,
    args: &[String],
    go_flags: &[String],
    heading: &str,
    target: stdlib::Target,
) -> i32 {
    if let Err(e) = go_cli::build_binary(build_dir, output_path, target, go_flags) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }

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
    if let Err(code) = crate::go_cli::require_go() {
        return code;
    }

    let target = target.unwrap_or_else(|| ".".to_string());

    if target.ends_with(".lis") {
        run_standalone(&target, args, sourcemap, &go_flags)
    } else {
        run_project(&target, args, sourcemap, &go_flags)
    }
}

fn run_project(path: &str, args: Vec<String>, sourcemap: bool, go_flags: &[String]) -> i32 {
    let project_path = Path::new(path);

    let prep = match super::build::prepare_project_build(project_path) {
        Ok(p) => p,
        Err(code) => return code,
    };

    // Held through the child's execution too: releasing sooner would let a concurrent
    // `lis build`/`sync`/LSP relink `target/` under the running program.
    let _target_lock = match crate::lock::acquire_target_lock(&prep.target_dir) {
        Ok(f) => f,
        Err(code) => return code,
    };

    let heading = "Failed to run project";
    let target = stdlib::Target::host();
    let binary_name = go_cli::binary_name(&prep.manifest.project.name, target);

    let build_result = super::build::build_locked(&prep, sourcemap, true, "Build");
    if build_result != 0 {
        return build_result;
    }

    let build_dir = match prep.target_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to resolve `{}`: {}", prep.target_dir.display(), e),
                "Check that the directory exists"
            );
            return 1;
        }
    };
    let output_path = build_dir.join("bin").join(&binary_name);

    build_and_exec(&build_dir, &output_path, &args, go_flags, heading, target)
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

    let compile_config = CompileConfig {
        target_phase: CompilePhase::Emit,
        go_module: "lis-standalone".to_string(),
        standalone_mode: true,
        load_siblings: false,
        sourcemap,
        project_root: None,
        locator: deps::TypedefLocator::default(),
    };

    let entry_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file)
        .to_string();
    let entry_display = lisette::fs::relative_to_cwd(file_path).unwrap_or_else(|| file.to_string());

    let no_loader = MemoryLoader::new();
    let result = compile(
        &source,
        &entry_name,
        &entry_display,
        &compile_config,
        &no_loader,
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
        &source,
        &entry_display,
    );

    if counts.errors > 0 {
        return 1;
    }

    if let Err(e) = go_cli::write_go_mod(&temp_dir, "lis-standalone", &compile_config.locator) {
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

    let target = compile_config.locator.target();
    let import_set_hash = go_cli::compute_import_set_hash(&emit.new_manifest, "lis-standalone");

    if let Err(e) = go_cli::finalize_go_dir(&temp_dir, target, &emit.changed, import_set_hash) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }

    go_cli::write_emit_manifest(&temp_dir, &emit.new_manifest);

    let output_path = temp_dir.join(go_cli::run_binary_name(target));
    build_and_exec(&temp_dir, &output_path, &args, go_flags, heading, target)
}
