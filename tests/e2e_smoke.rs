use std::process::Command;

#[test]
fn e2e_smoke() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping e2e_smoke: `go` not found");
        return;
    }

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let e2e_dir = repo.join("tests/e2e_smoke_project");
    let target_dir = e2e_dir.join("target");

    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).expect("failed to clean target/");
    }

    let run = Command::new("cargo")
        .args(["run", "-p", "lisette", "--quiet", "--", "test"])
        .arg(&e2e_dir)
        .current_dir(repo)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lisette test");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    let output = format!("{}{}", stdout, stderr);

    assert!(
        run.status.success(),
        "lis test failed (exit {:?}):\n{}",
        run.status.code(),
        output
    );
    assert!(
        !output.contains("▲"),
        "lis test produced warnings:\n{}",
        output
    );
}
