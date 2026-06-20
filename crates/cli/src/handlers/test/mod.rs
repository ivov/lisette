use std::time::Instant;

use crate::cli_error;
use crate::go_cli;
use crate::output::use_color;

use super::build::{BuildOptions, build_locked, with_locked_project};

use std::collections::BTreeMap;

mod report;
use report::{build_report_filtered, exit_code, matching_tests, render};

pub fn test(path: Option<String>, go_flags: Vec<String>, filter: Option<String>) -> i32 {
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

        let scopes = match filter.as_deref() {
            Some(pattern) => {
                let matched =
                    matching_tests(&outcome.test_index, &prep.manifest.project.name, pattern);
                if matched.is_empty() {
                    eprintln!("\n  No tests match `{pattern}`\n");
                    return 0;
                }
                let mut by_package: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (package, go_name) in matched {
                    by_package.entry(package).or_default().push(go_name);
                }
                Some(
                    by_package
                        .into_iter()
                        .map(|(package, names)| (package, format!("^({})$", names.join("|"))))
                        .collect::<Vec<_>>(),
                )
            }
            None => None,
        };

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
        let run = match go_cli::run_tests(
            &build_dir,
            stdlib::Target::host(),
            &go_flags,
            scopes.as_deref(),
        ) {
            Ok(run) => run,
            Err(e) => {
                cli_error!("Failed to run tests", e.message, e.hint);
                return 1;
            }
        };

        let report = build_report_filtered(
            &outcome.test_index,
            &run.events,
            &prep.manifest.project.name,
            filter.as_deref(),
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
