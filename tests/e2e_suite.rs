#[allow(dead_code, unused_imports)]
mod _harness;
#[path = "e2e_suite/harness.rs"]
mod harness;

use std::fs;
use std::process::Command;

use rayon::prelude::*;

use harness::{
    EmittedTest, HarvestedTest, compile_e2e_suite_test, harvest_snapshots, prelude_dir,
    read_go_version, read_skip_list, run_go_test, run_go_vet, skip_reason_for_imports,
    snapshots_dir, target_dir, write_go_mod, write_subpackage,
};

#[test]
fn e2e_suite() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping e2e_suite: `go` not found");
        return;
    }

    let snapshots = snapshots_dir();
    let target = target_dir();
    let prelude = prelude_dir();

    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).expect("create target/e2e_suite");

    let go_version = read_go_version().expect("read go-version");
    write_go_mod(&target, &prelude, &go_version).expect("write go.mod");

    let harvested = harvest_snapshots(&snapshots);
    assert!(
        !harvested.is_empty(),
        "no snapshots harvested from {}",
        snapshots.display()
    );

    let skip_list = read_skip_list();

    enum Outcome {
        Denylist(String),
        SkippedImport(String, String),
        EmitFailure(String, String),
        Included(String),
        BuildOnly(String),
    }

    let outcomes: Vec<Outcome> = harvested
        .par_iter()
        .map(
            |HarvestedTest {
                 name,
                 input,
                 snap_body,
             }| {
                if skip_list.contains(name) {
                    return Outcome::Denylist(name.clone());
                }
                if let Some(reason) = skip_reason_for_imports(snap_body) {
                    return Outcome::SkippedImport(name.clone(), reason);
                }
                let EmittedTest { go_code, entry } =
                    match compile_e2e_suite_test(input, &format!("test_{name}")) {
                        Ok(emitted) => emitted,
                        Err(error) => return Outcome::EmitFailure(name.clone(), error),
                    };
                write_subpackage(&target, name, &go_code, entry).expect("write subpackage");
                if entry.is_some() {
                    Outcome::Included(name.clone())
                } else {
                    Outcome::BuildOnly(name.clone())
                }
            },
        )
        .collect();

    let mut emit_failures = Vec::new();
    let mut skipped_imports = Vec::new();
    let mut skipped_denylist = Vec::new();
    let mut build_only = Vec::new();
    let mut included = Vec::new();
    for outcome in outcomes {
        match outcome {
            Outcome::Denylist(name) => skipped_denylist.push(name),
            Outcome::SkippedImport(name, reason) => skipped_imports.push((name, reason)),
            Outcome::EmitFailure(name, error) => emit_failures.push((name, error)),
            Outcome::Included(name) => included.push(name),
            Outcome::BuildOnly(name) => build_only.push(name),
        }
    }

    eprintln!(
        "harvested {}, included {} (run), {} (build-only), skipped {} (imports), {} (deny-list), {} re-emit failures",
        harvested.len(),
        included.len(),
        build_only.len(),
        skipped_imports.len(),
        skipped_denylist.len(),
        emit_failures.len(),
    );

    if !emit_failures.is_empty() {
        for (name, err) in &emit_failures {
            eprintln!("  re-emit failed: {name}: {err}");
        }
    }

    assert!(
        emit_failures.is_empty(),
        "{} snapshot(s) failed to re-emit (listed above); fix each snapshot or, if it cannot run as a self-contained Go test, add its stem to tests/e2e_suite/skip.txt",
        emit_failures.len(),
    );

    assert!(!included.is_empty(), "no tests included");

    eprintln!("running `go test ./...` in {}", target.display());
    match run_go_test(&target, "30s") {
        Ok(out) => {
            eprintln!("{out}");
        }
        Err(out) => {
            eprintln!("{out}");
            panic!("go test failed");
        }
    }

    eprintln!("running `go vet ./...` in {}", target.display());
    if let Err(out) = run_go_vet(&target) {
        eprintln!("{out}");
        panic!("go vet failed");
    }
}
