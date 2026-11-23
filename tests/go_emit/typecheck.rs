//! Accepted programs that `go_emit` does not build must give Go that `go/types` accepts.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use rayon::prelude::*;

use crate::_harness::go::go_available;
use crate::_harness::go::{
    CompileError, EmittedTest, HarvestedTest, compile_go_test, harvest_snapshots, prelude_dir,
    read_go_toolchain, repo_root, skip_reason_for_imports, snapshots_dir, write_go_mod,
    write_subpackage,
};
use crate::_harness::go_types::{fresh_dir, panic_message, type_check_module};
use crate::_harness::lists::{FailingList, read_names};

struct AcceptedProgram {
    name: String,
    input: String,
}

enum Outcome {
    SkippedImport,
    Rejected,
    EmitPanic(String),
    Written,
}

#[test]
fn infer_programs_give_valid_go() {
    if !go_available() {
        eprintln!("skipping infer_programs_give_valid_go: `go` not found");
        return;
    }
    let programs = harvest_accepted_infer_inputs(&repo_root().join("tests/fast_suite/spec/infer"));
    assert!(!programs.is_empty(), "no infer inputs harvested");
    type_check_programs(
        "infer_inputs",
        &programs,
        &FailingList::read("go_emit/failing_infer.txt"),
    );
}

#[test]
fn typecheck_only_snapshots_give_valid_go() {
    if !go_available() {
        eprintln!("skipping typecheck_only_snapshots_give_valid_go: `go` not found");
        return;
    }
    let typecheck_only = read_names("go_emit/typecheck_only.txt");
    let programs: Vec<AcceptedProgram> = harvest_snapshots(&snapshots_dir("emit"))
        .into_iter()
        .filter(|HarvestedTest { name, .. }| typecheck_only.contains(name))
        .map(|HarvestedTest { name, input, .. }| AcceptedProgram { name, input })
        .collect();
    assert!(
        !programs.is_empty(),
        "no type-check-only emit snapshots harvested"
    );
    type_check_programs(
        "typecheck_only_snapshots",
        &programs,
        &FailingList::read("go_emit/failing_snapshots.txt"),
    );
}

fn type_check_programs(label: &str, programs: &[AcceptedProgram], failing: &FailingList) {
    let target = repo_root().join("target/go_emit_typecheck").join(label);
    fresh_dir(&target);
    write_go_mod(&target, &prelude_dir(), &read_go_toolchain().unwrap()).unwrap();

    let outcomes: Vec<(&str, Outcome)> = programs
        .par_iter()
        .map(|AcceptedProgram { name, input }| {
            let compiled = panic::catch_unwind(AssertUnwindSafe(|| {
                compile_go_test(input, &format!("test_{name}"))
            }));
            let outcome = match compiled {
                Err(payload) => Outcome::EmitPanic(panic_message(payload.as_ref())),
                Ok(Err(CompileError::Inference(_) | CompileError::Emit(_))) => Outcome::Rejected,
                Ok(Ok(EmittedTest { go_code, .. })) => {
                    if skip_reason_for_imports(input, &go_code).is_some() {
                        Outcome::SkippedImport
                    } else {
                        write_subpackage(&target, name, &go_code, None).unwrap();
                        Outcome::Written
                    }
                }
            };
            (name.as_str(), outcome)
        })
        .collect();

    let mut ran = BTreeSet::new();
    let mut failures = BTreeMap::new();
    let (mut written, mut rejected, mut skipped) = (0, 0, 0);
    for (name, outcome) in outcomes {
        match outcome {
            Outcome::SkippedImport => {
                skipped += 1;
                continue;
            }
            Outcome::Rejected => rejected += 1,
            Outcome::Written => written += 1,
            Outcome::EmitPanic(message) => {
                failures.insert(name.to_string(), format!("emit panic: {message}"));
            }
        }
        ran.insert(name.to_string());
    }

    for (package, errors) in type_check_module(&target) {
        let name = package.strip_prefix("test_").unwrap_or(&package);
        failures
            .entry(name.to_string())
            .or_insert_with(|| format!("go/types: {}", errors.join(" | ")));
    }

    eprintln!(
        "{label}: {} programs, {written} type-checked, {rejected} rejected by inference or emit, {skipped} skipped (non-stdlib imports)",
        programs.len()
    );
    failing.assert_matches(&ran, &failures, true);
}

/// Functions that only the infer harness declares, and `@rawgo`, whose Go text the harness does not import for.
const HARNESS_ONLY_NAMES: &[&str] = &["get_unknown", "takes_unknown", "@rawgo"];

/// Each `infer(r#"..."#).assert_no_errors()` call in `tests/fast_suite/spec/infer`, except harness-only inputs.
fn harvest_accepted_infer_inputs(spec_dir: &Path) -> Vec<AcceptedProgram> {
    const CALL: &str = "infer(";
    const OPEN: &str = "r#\"";
    const CLOSE: &str = "\"#";
    let mut files: Vec<_> = fs::read_dir(spec_dir)
        .expect("read tests/fast_suite/spec/infer")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    files.sort();
    let mut programs = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let source = fs::read_to_string(&path).expect("read spec file");
        let mut offset = 0;
        while let Some(found) = source[offset..].find(CALL) {
            let at = offset + found;
            offset = at + CALL.len();
            let Some(body) = source[offset..].trim_start().strip_prefix(OPEN) else {
                continue;
            };
            let Some(end) = body.find(CLOSE) else {
                break;
            };
            let tail = body[end + CLOSE.len()..].trim_start();
            let tail = tail.strip_prefix(',').unwrap_or(tail).trim_start();
            let Some(tail) = tail.strip_prefix(')') else {
                continue;
            };
            let input = &body[..end];
            if !tail.trim_start().starts_with(".assert_no_errors()")
                || HARNESS_ONLY_NAMES.iter().any(|name| input.contains(name))
            {
                continue;
            }
            let test_name = source[..at]
                .rfind("\nfn ")
                .map(|start| &source[start + 4..at])
                .and_then(|header| header.split('(').next())
                .unwrap_or("unnamed");
            let base = format!("{stem}__{test_name}");
            let count = seen.entry(base.clone()).or_insert(0);
            *count += 1;
            let name = if *count == 1 {
                base
            } else {
                format!("{base}_{count}")
            };
            programs.push(AcceptedProgram {
                name,
                input: input.to_string(),
            });
        }
    }
    programs
}
