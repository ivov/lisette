use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};
use std::process::Command;

use crate::harness::{
    GO_MODULE, parse_snap_body, prelude_dir, read_go_toolchain, repo_root, run_go_test, run_go_vet,
    run_gofmt_simplify, snapshots_dir, write_go_mod,
};

enum Check {
    Compile,
    Vet,
}

// Fixing these diagnostics requires separately reviewed snapshot changes.
const KNOWN_FAILURES: &[(&str, Check, &str)] = &[
    (
        "go_method_option_comma_ok_wrapped",
        Check::Compile,
        "./main.go:6:2: \"time\" imported and not used",
    ),
    (
        "go_exported_impl_method_kept_even_when_never_called",
        Check::Vet,
        "main.go:7:17: method MarshalJSON() string should have signature MarshalJSON() ([]byte, error)",
    ),
    (
        "user_fmt_alias_preserved_alongside_generated_import",
        Check::Vet,
        "main.go:22:2: result of fmt.Sprintf call not used",
    ),
    (
        "user_marshal_json_lowers_to_go_abi_shape",
        Check::Vet,
        "main.go:12:17: method MarshalJSON() ([]uint8, error) should have signature MarshalJSON() ([]byte, error)",
    ),
];

#[test]
fn build_snapshots_compile() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping build snapshots: `go` not found");
        return;
    }

    let target = repo_root().join("target/e2e_build_suite");
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).expect("create build snapshot target");
    write_go_mod(&target, &prelude_dir(), &read_go_toolchain().unwrap()).unwrap();

    let mut harvested = 0;
    let mut known_failures = Vec::new();
    for entry in fs::read_dir(snapshots_dir("build")).expect("read build snapshots") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("snap") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        let content = fs::read_to_string(&path).expect("read build snapshot");
        let body = parse_snap_body(&content).expect("snapshot body");
        let files = parse_files(&body);
        assert!(!files.is_empty(), "no Go files in {}", path.display());

        let known = KNOWN_FAILURES
            .iter()
            .find(|(snapshot, ..)| *snapshot == name);
        let relative = if known.is_some() {
            format!("_known_failures/{name}")
        } else {
            format!("build/{name}")
        };
        let directory = target.join(&relative);
        for (file, code) in files {
            let destination = directory.join(file);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            // Each snapshot's local packages need their own import namespace.
            let code = code.replace(
                "\"github.com/user/myproject/",
                &format!("\"{GO_MODULE}/{relative}/"),
            );
            fs::write(destination, code).unwrap();
        }
        if name == "assert_type_emits_concrete_type_arg" {
            // The fixture declares store.d.lis without a Go implementation.
            fs::create_dir_all(directory.join("store")).unwrap();
            fs::write(
                directory.join("store/store.go"),
                include_str!("fixtures/store.go"),
            )
            .unwrap();
        }
        if let Some(known) = known {
            known_failures.push((directory, known));
        }
        harvested += 1;
    }
    assert!(harvested > 0, "no build snapshots harvested");

    assert_eq!(
        known_failures.len(),
        KNOWN_FAILURES.len(),
        "known snapshots exist"
    );
    for (directory, (name, check, expected)) in known_failures {
        let result = match check {
            Check::Compile => run_go_test(&directory, "30s"),
            Check::Vet => {
                run_go_test(&directory, "30s")
                    .unwrap_or_else(|out| panic!("{name} no longer compiles:\n{out}"));
                run_go_vet(&directory)
            }
        };
        let failure = result.expect_err("check now passes; remove its expected failure");
        let diagnostics: Vec<_> = failure
            .lines()
            .filter(|line| line.contains(".go:"))
            .collect();
        assert_eq!(
            diagnostics,
            [*expected],
            "unexpected failure in {name}:\n{failure}"
        );
        eprintln!("known Go diagnostic in {name}: {failure}");
    }
    eprintln!(
        "checking {harvested} build snapshots, including {} expected diagnostics",
        KNOWN_FAILURES.len()
    );

    run_go_test(&target, "30s").unwrap_or_else(|out| panic!("go test failed:\n{out}"));
    run_go_vet(&target).unwrap_or_else(|out| panic!("go vet failed:\n{out}"));
    run_gofmt_simplify(&target).unwrap_or_else(|out| panic!("gofmt -s would rewrite:\n{out}"));
}

fn parse_files(body: &str) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut current = None;
    for line in body.lines() {
        if let Some(file) = line
            .strip_prefix("// === ")
            .and_then(|line| line.strip_suffix(" ==="))
        {
            assert!(
                Path::new(file)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                    && file.ends_with(".go"),
                "invalid snapshot file path: {file}",
            );
            assert!(
                files.insert(file.to_string(), String::new()).is_none(),
                "duplicate file: {file}"
            );
            current = Some(file);
        } else if let Some(file) = current {
            let code = files.get_mut(file).unwrap();
            code.push_str(line);
            code.push('\n');
        } else {
            assert!(line.trim().is_empty(), "Go code before a file marker");
        }
    }
    for code in files.values_mut() {
        code.truncate(code.trim_end().len());
        code.push('\n');
    }
    files
}
