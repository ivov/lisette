use std::time::Instant;

use crate::cli_error;
use crate::go_cli;
use crate::output::use_color;

use super::build::{BuildOptions, build_locked, with_locked_project};

mod report;
use report::{build_report, exit_code, render};

pub fn test(path: Option<String>, go_flags: Vec<String>) -> i32 {
    crate::output::print_test_unfinished_notice();
    with_locked_project(path, |prep| {
        let outcome = build_locked(
            prep,
            BuildOptions {
                sourcemap: false,
                quiet: false,
                emit_tests: true,
                label: "Compiled",
            },
        );
        if outcome.code != 0 {
            return outcome.code;
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

        let started = Instant::now();
        let run = match go_cli::run_tests(&build_dir, stdlib::Target::host(), &go_flags) {
            Ok(run) => run,
            Err(e) => {
                cli_error!("Failed to run tests", e.message, e.hint);
                return 1;
            }
        };

        let report = build_report(
            &outcome.test_index,
            &run.events,
            &prep.manifest.project.name,
        );
        eprint!(
            "{}",
            render(&report, &outcome.sources, use_color(), started.elapsed())
        );

        let build_error = report.build_output.trim();
        if !build_error.is_empty() {
            cli_error!(
                "Tests could not run",
                build_error.to_string(),
                "The generated Go failed to build; run `lis check`"
            );
        }

        exit_code(&report.rows, run.success)
    })
}
