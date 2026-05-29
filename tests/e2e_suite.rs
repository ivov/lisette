#[allow(dead_code, unused_imports)]
mod _harness;
#[path = "e2e_suite/harness.rs"]
mod harness;

use std::fs;
use std::process::Command;

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

    let mut emit_failures = Vec::new();
    let mut skipped_imports = Vec::new();
    let mut skipped_denylist = Vec::new();
    let mut skipped_no_entry = Vec::new();
    let mut included = Vec::new();

    for HarvestedTest {
        name,
        input,
        snap_body,
    } in &harvested
    {
        if skip_list.contains(name) {
            skipped_denylist.push(name.clone());
            continue;
        }
        if let Some(reason) = skip_reason_for_imports(snap_body) {
            skipped_imports.push((name.clone(), reason));
            continue;
        }
        let result = match compile_e2e_suite_test(input, &format!("test_{name}")) {
            Ok(r) => r,
            Err(e) => {
                emit_failures.push((name.clone(), e));
                continue;
            }
        };
        let EmittedTest { go_code, entry } = result;

        let Some(entry) = entry else {
            skipped_no_entry.push(name.clone());
            continue;
        };
        write_subpackage(&target, name, &go_code, entry).expect("write subpackage");
        included.push(name.clone());
    }

    eprintln!(
        "harvested {}, included {}, skipped {} (imports), {} (deny-list), {} (no entry), {} re-emit failures",
        harvested.len(),
        included.len(),
        skipped_imports.len(),
        skipped_denylist.len(),
        skipped_no_entry.len(),
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
