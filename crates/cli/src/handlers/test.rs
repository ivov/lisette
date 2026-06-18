use crate::cli_error;
use crate::go_cli;

use super::build::{BuildOptions, build_locked, with_locked_project};

pub fn test(path: Option<String>, go_flags: Vec<String>) -> i32 {
    crate::output::print_test_unfinished_notice();
    with_locked_project(path, |prep| {
        let emit_code = build_locked(
            prep,
            BuildOptions {
                sourcemap: false,
                quiet: false,
                emit_tests: true,
                label: "Compiled",
            },
        );
        if emit_code != 0 {
            return emit_code;
        }

        let build_dir = match prep.target_dir.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                cli_error!(
                    "Failed to run tests",
                    format!("Failed to resolve `{}`: {}", prep.target_dir.display(), e),
                    "Check that the directory exists"
                );
                return 1;
            }
        };

        let test_start = std::time::Instant::now();
        match go_cli::run_tests(&build_dir, stdlib::Target::host(), &go_flags) {
            Ok(true) => {
                eprintln!(
                    "  ✓ Tests passed {}",
                    crate::output::format_elapsed(test_start.elapsed())
                );
                0
            }
            Ok(false) => {
                eprintln!(
                    "  ✗ Tests failed {}",
                    crate::output::format_elapsed(test_start.elapsed())
                );
                1
            }
            Err(e) => {
                cli_error!("Failed to run tests", e.message, e.hint);
                1
            }
        }
    })
}
