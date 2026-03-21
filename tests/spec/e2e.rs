use std::process::Command;

#[test]
fn e2e_suite() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping e2e: `go` not found");
        return;
    }

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let e2e_dir = repo.join("tests/e2e");
    let target_dir = e2e_dir.join("target");

    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).expect("failed to clean target/");
    }

    let build = Command::new("cargo")
        .args(["run", "-p", "lisette", "--quiet", "--", "build"])
        .arg(&e2e_dir)
        .current_dir(repo)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lisette build");

    let build_stderr = String::from_utf8_lossy(&build.stderr);
    let build_stdout = String::from_utf8_lossy(&build.stdout);
    let build_output = format!("{}{}", build_stdout, build_stderr);

    assert!(
        build.status.success(),
        "lisette build failed:\n{}",
        build_output
    );
    assert!(
        !build_output.contains("[warning]"),
        "build produced warnings:\n{}",
        build_output
    );

    let run = Command::new("go")
        .args(["run", "."])
        .current_dir(&target_dir)
        .output()
        .expect("failed to run go");

    assert!(
        run.status.success(),
        "go run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let stdout = String::from_utf8_lossy(&run.stdout);
    insta::with_settings!({
        snapshot_path => "emit/snapshots",
        snapshot_suffix => "",
        prepend_module_to_snapshot => false,
    }, {
        insta::assert_snapshot!("e2e", stdout);
    });
}
