#[allow(dead_code, unused_imports)]
mod _harness;
mod emit_runtime_harness;

use std::fs;
use std::process::Command;

use emit_runtime_harness::{
    EmittedTest, HarvestedTest, compile_emit_runtime_test, harvest_snapshots, prelude_dir,
    read_go_version, read_skip_list, run_go_test, skip_reason_for_imports, snapshots_dir,
    target_dir, write_go_mod, write_subpackage,
};

#[test]
fn emit_runtime_suite() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping emit_runtime: `go` not found");
        return;
    }

    let snapshots = snapshots_dir();
    let target = target_dir();
    let prelude = prelude_dir();

    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).expect("create target/emit_runtime");

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
        let result = match compile_emit_runtime_test(input, &format!("test_{name}")) {
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
        "harvested {}, included {}, skipped {} (imports), {} (deny-list), {} (no entry), {} re-emit failures (orphaned snaps or stale descriptions; investigate via `cargo insta test --unreferenced reject`)",
        harvested.len(),
        included.len(),
        skipped_imports.len(),
        skipped_denylist.len(),
        skipped_no_entry.len(),
        emit_failures.len(),
    );

    if !emit_failures.is_empty() {
        for (name, err) in emit_failures.iter().take(10) {
            eprintln!("  re-emit failed: {name}: {err}");
        }
    }

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
}
