use std::process::Command;

use tempfile::TempDir;

fn go_available() -> bool {
    Command::new("go").arg("version").output().is_ok()
}

#[test]
fn e2e_learn() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();

    let build = Command::new("cargo")
        .args(["build", "-p", "lisette", "--quiet"])
        .current_dir(repo)
        .env("NO_COLOR", "1")
        .status()
        .expect("failed to build lisette");
    assert!(build.success(), "cargo build -p lisette failed");

    let lis = repo.join("target/debug/lis");
    let temp = TempDir::new().expect("failed to create temp dir");

    let learn = Command::new(&lis)
        .arg("learn")
        .current_dir(temp.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lis learn");
    assert!(
        learn.status.success(),
        "lis learn failed:\n{}",
        String::from_utf8_lossy(&learn.stderr)
    );

    let project = temp.path().join("learn-lisette");
    assert!(
        project.join("lisette.toml").exists() && project.join("src/main.lis").exists(),
        "lis learn did not scaffold the expected src/ layout"
    );

    let check = Command::new(&lis)
        .args(["check", "--output", "unix"])
        .arg(&project)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lis check");
    let check_stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        check.status.success() && check_stdout.trim().is_empty(),
        "lis check reported issues on the scaffolded learn project:\n{}{}",
        check_stdout,
        String::from_utf8_lossy(&check.stderr)
    );

    let format = Command::new(&lis)
        .args(["format", "--check"])
        .arg(&project)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lis format --check");
    assert!(
        format.status.success(),
        "lis format --check reported unformatted files in the scaffolded learn project:\n{}{}",
        String::from_utf8_lossy(&format.stdout),
        String::from_utf8_lossy(&format.stderr)
    );

    if !go_available() {
        eprintln!("skipping `lis test` step of e2e_learn: `go` not found");
        return;
    }

    let test = Command::new(&lis)
        .arg("test")
        .arg(&project)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lis test");
    let test_output = format!(
        "{}{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    assert!(
        test.status.success(),
        "lis test failed on the scaffolded learn project:\n{test_output}"
    );
    assert!(
        test_output.contains("passed") && !test_output.contains("No tests found"),
        "lis test did not run the scaffolded tests:\n{test_output}"
    );
}
