use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use crate::cli_error;
use crate::go_cli;
use crate::output::{terminal_width, use_color};

use super::build::{BuildOptions, build_locked, with_locked_project};

mod failed;
mod report;
use report::{
    all_test_keys, build_report_filtered, exit_code, matching_tests, nothing_executed, render,
};

pub fn test(
    path: Option<String>,
    go_flags: Vec<String>,
    filter: Option<String>,
    failed: bool,
) -> i32 {
    crate::output::print_preview_notice("Test runner", false);
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

        let go_module = &prep.manifest.project.name;

        let selected: Option<HashSet<(String, String)>> = if failed {
            let live = all_test_keys(&outcome.test_index, go_module);
            let set: HashSet<(String, String)> = failed::load(&prep.target_dir)
                .into_iter()
                .filter(|key| live.contains(key))
                .collect();
            if set.is_empty() {
                let message = crate::output::format_backticks(
                    "No failures to rerun. Run `lis test` first.",
                    use_color(),
                );
                eprintln!("\n  {message}\n");
                return 0;
            }
            Some(set)
        } else if let Some(pattern) = filter.as_deref() {
            let matched = matching_tests(&outcome.test_index, go_module, pattern);
            if matched.is_empty() {
                let message = crate::output::format_backticks(
                    &format!("No tests match `{pattern}`"),
                    use_color(),
                );
                eprintln!("\n  {message}\n");
                return 0;
            }
            Some(matched.into_iter().collect())
        } else {
            None
        };

        let scopes = selected.as_ref().map(|set| {
            let mut by_package: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (package, go_name) in set {
                by_package
                    .entry(package.clone())
                    .or_default()
                    .push(go_name.clone());
            }
            by_package
                .into_iter()
                .map(|(package, mut names)| {
                    names.sort();
                    (package, format!("^({})$", names.join("|")))
                })
                .collect::<Vec<_>>()
        });

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
            go_module,
            selected.as_ref(),
        );
        print!(
            "{}",
            render(
                &report,
                &outcome.sources,
                use_color(),
                Duration::from_secs_f64(report.test_elapsed),
                terminal_width(),
            )
        );

        let build_error = report.build_output.trim();
        if !build_error.is_empty() {
            cli_error!(
                "Tests could not run",
                build_error.to_string(),
                "The generated Go failed to build; run `lis check`"
            );
        } else if filter.is_none() && !nothing_executed(&report.rows) {
            failed::save(&prep.target_dir, &report.rows);
        }

        exit_code(&report.rows, run.success)
    })
}
