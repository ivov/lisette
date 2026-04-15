use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use crate::cli_error;
use crate::go_cli;
use diagnostics::render::{self, Filter};
use lisette::pipeline::{CompileConfig, CompilePhase, compile};
use semantics::loader::Loader;

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

    let project_path = Path::new(path);
    let target_dir = project_path.join("target");

    let mut cmd = Command::new("go");
    cmd.arg("run").arg("-C").arg(&target_dir).arg(".");
    cmd.args(&args);

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            cli_error!(
                "Failed to run project",
                format!("Failed to execute `go run`: {}", e),
                "Check Go installation with `go version`"
            );
            1
        }
    }
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

    for file in &result.output {
        let go_file_path = temp_dir.join(&file.name);
        let go_code = file.to_go();

        if let Some(parent) = go_file_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            cli_error!(
                "Failed to run standalone file",
                format!("Failed to create directory `{}`: {}", parent.display(), e),
                "Check directory permissions"
            );
            return 1;
        }

        if let Err(e) = fs::write(&go_file_path, &go_code) {
            cli_error!(
                "Failed to run standalone file",
                format!("Failed to write `{}`: {}", go_file_path.display(), e),
                "Check file permissions"
            );
            return 1;
        }
    }

    if let Err(e) = go_cli::go_fmt(&temp_dir) {
        cli_error!(
            "Failed to run standalone file",
            format!("Go format failed: {}", e),
            "Check Go installation with `go version`"
        );
        return 1;
    }

    if let Err(e) = go_cli::ensure_go_sum(&temp_dir) {
        cli_error!(
            "Failed to run standalone file",
            format!("Failed to resolve Go dependencies: {}", e),
            "Check Go installation and network connectivity"
        );
        return 1;
    }

    let mut cmd = Command::new("go");
    cmd.arg("run").arg("-C").arg(&temp_dir).arg(".");
    cmd.args(&args);

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            cli_error!(
                "Failed to run standalone file",
                format!("Failed to execute `go run`: {}", e),
                "Check Go installation with `go version`"
            );
            1
        }
    }
}
