#[allow(dead_code, unused_imports)]
mod _harness;

use _harness::go::go_available;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn language_tests_pass() {
    if !go_available() {
        eprintln!("skipping language_tests_pass: `go` not found");
        return;
    }

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let project_dir = repo.join("tests/cli_language/project");
    let target_dir = project_dir.join("target");

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).expect("failed to clean target/");
    }

    let run = Command::new("cargo")
        .args(["run", "-p", "lisette", "--quiet", "--", "test"])
        .arg(&project_dir)
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
