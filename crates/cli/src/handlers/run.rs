use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
#[cfg(not(unix))]
use std::path::PathBuf;
#[cfg(not(unix))]
use std::process::Command;

use crate::cli_error;
use crate::go_cli;
use diagnostics::render::{self, Filter};
use lisette::pipeline::{CompileConfig, CompilePhase, compile};
use semantics::loader::Loader;

fn run_with_invocation_cwd(build_dir: &Path, args: &[String], heading: &str) -> i32 {
    #[cfg(unix)]
    {
        run_via_exec_wrapper(build_dir, args, heading)
    }
    #[cfg(not(unix))]
    {
        build_then_exec(build_dir, args, heading)
    }
}

#[cfg(unix)]
fn run_via_exec_wrapper(build_dir: &Path, args: &[String], heading: &str) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to read current directory: {}", e),
                "Check directory permissions"
            );
            return 1;
        }
    };
    let cwd_str = cwd.to_string_lossy();
    let quoted_cwd = match quote_for_go_exec(&cwd_str) {
        Ok(q) => q,
        Err(e) => {
            cli_error!(
                heading,
                e,
                "Run from a directory whose path does not contain both `'` and `\"`"
            );
            return 1;
        }
    };

    let mut cmd = go_cli::go_command();
    cmd.arg("-C")
        .arg(build_dir)
        .arg("run")
        .arg(format!("-exec=env -C {}", quoted_cwd))
        .arg(".")
        .args(args);

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to execute `go run`: {}", e),
                "Check Go installation with `go version`"
            );
            1
        }
    }
}

#[cfg(unix)]
fn quote_for_go_exec(path: &str) -> Result<String, String> {
    let has_single = path.contains('\'');
    let has_double = path.contains('"');
    match (has_single, has_double) {
        (true, true) => Err(format!(
            "Cannot pass cwd `{}` through `go run -exec`: contains both `'` and `\"`",
            path
        )),
        (true, false) => Ok(format!("\"{}\"", path)),
        _ => Ok(format!("'{}'", path)),
    }
}

#[cfg(not(unix))]
const RUN_BIN_NAME: &str = "lis-run.exe";

#[cfg(not(unix))]
fn build_then_exec(build_dir: &Path, args: &[String], heading: &str) -> i32 {
    let abs_build_dir = match build_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to resolve `{}`: {}", build_dir.display(), e),
                "Check that the directory exists"
            );
            return 1;
        }
    };
    let binary_path: PathBuf = abs_build_dir.join(RUN_BIN_NAME);

    let mut build_cmd = go_cli::go_command();
    build_cmd
        .arg("build")
        .arg("-o")
        .arg(&binary_path)
        .arg(".")
        .current_dir(&abs_build_dir);

    match build_cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => return s.code().unwrap_or(1),
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to execute `go build`: {}", e),
                "Check Go installation with `go version`"
            );
            return 1;
        }
    }

    let mut cmd = Command::new(&binary_path);
    cmd.args(args);

    match cmd.status() {
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

pub fn run(target: Option<String>, args: Vec<String>, debug: bool) -> i32 {
    if let Err(code) = crate::go_cli::require_go() {
        return code;
    }

    let target = target.unwrap_or_else(|| ".".to_string());

    if target.ends_with(".lis") {
        run_standalone(&target, args, debug)
    } else {
        run_project(&target, args, debug)
    }
}

fn run_project(path: &str, args: Vec<String>, debug: bool) -> i32 {
    let build_result = crate::handlers::build(Some(path.to_string()), debug, true);
    if build_result != 0 {
        return build_result;
    }

    let target_dir = Path::new(path).join("target");
    run_with_invocation_cwd(&target_dir, &args, "Failed to run project")
}

fn run_standalone(file: &str, args: Vec<String>, debug: bool) -> i32 {
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

    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("main.lis");

    let compile_config = CompileConfig {
        target_phase: CompilePhase::Emit,
        go_module: "lis-standalone".to_string(),
        standalone_mode: true,
        load_siblings: false,
        debug,
        project_root: None,
        locator: deps::TypedefLocator::default(),
    };

    struct NoLoader;
    impl Loader for NoLoader {
        fn scan_folder(&self, _folder_name: &str) -> rustc_hash::FxHashMap<String, String> {
            rustc_hash::FxHashMap::default()
        }
    }

    let no_loader = NoLoader;
    let result = compile(&source, filename, &compile_config, &no_loader);

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
        file,
    );

    if counts.errors > 0 {
        return 1;
    }

    if let Err(e) = go_cli::write_go_mod(&temp_dir, "lis-standalone", &compile_config.locator) {
        cli_error!("Failed to run standalone file", e, "Check file permissions");
        return 1;
    }

    let heading = "Failed to run standalone file";

    if let Err(code) = go_cli::write_go_outputs(&temp_dir, &result.output, heading) {
        return code;
    }

    if let Err(code) = go_cli::finalize_go_dir(&temp_dir, heading) {
        return code;
    }

    run_with_invocation_cwd(&temp_dir, &args, "Failed to run standalone file")
}
