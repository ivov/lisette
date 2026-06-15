use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn go_available() -> bool {
    Command::new("go").arg("version").output().is_ok()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn scaffold_marker_project(root: &Path) -> (PathBuf, PathBuf) {
    let project = root.join("proj");
    let invocation = root.join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"cwdprobe\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"
import "go:os"

fn main() {
  match os.ReadFile("lis-run-cwd-marker") {
    Ok(_) => fmt.Println("FOUND_MARKER"),
    Err(_) => fmt.Println("NO_MARKER"),
  }
}
"#,
    )
    .unwrap();

    fs::write(invocation.join("lis-run-cwd-marker"), "ok").unwrap();

    (project, invocation)
}

fn lis_run(project: &Path, invocation: &Path, extra: &[&str]) -> std::process::Output {
    let manifest = repo().join("Cargo.toml");
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", "lisette", "--", "run"])
        .arg(project)
        .args(extra)
        .current_dir(invocation)
        .env("NO_COLOR", "1");
    cmd.output().expect("failed to invoke lisette")
}

#[test]
fn run_executes_binary_in_invocation_cwd() {
    if !go_available() {
        eprintln!("skipping run_executes_binary_in_invocation_cwd: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let (project, invocation) = scaffold_marker_project(scratch.path());

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("FOUND_MARKER"),
        "program did not resolve a relative path against the invocation cwd:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn run_forwards_go_flags() {
    if !go_available() {
        eprintln!("skipping run_forwards_go_flags: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let (project, invocation) = scaffold_marker_project(scratch.path());

    let output = lis_run(&project, &invocation, &["--go-flags", "-trimpath"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run --go-flags -trimpath failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("FOUND_MARKER"),
        "program output unexpected with --go-flags:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn run_unused_stronger_bound_does_not_constrain_type() {
    if !go_available() {
        eprintln!("skipping run_unused_stronger_bound_does_not_constrain_type: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    let invocation = scratch.path().join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"unusedstronger\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"

struct Point { x: int }

struct Box<T: Comparable> { value: T }

impl<T: Ordered> Box<T> {
  fn less(self, _other: Box<T>) -> bool { true }
}

fn main() {
  let _ = Box { value: Point { x: 1 } }
  fmt.Println("ok")
}
"#,
    )
    .unwrap();

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed (unused stronger bound likely hoisted into the type):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("ok"),
        "program did not run:\nstdout: {stdout}\nstderr: {stderr}"
    );
}
