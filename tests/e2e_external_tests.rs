use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn go_available() -> bool {
    Command::new("go").arg("version").output().is_ok()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn lis(project: &Path, args: &[&str]) -> Output {
    let manifest = repo().join("Cargo.toml");
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", "lisette", "--"])
        .args(args)
        .arg(project)
        .env("NO_COLOR", "1");
    cmd.output().expect("failed to invoke lisette")
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn scaffold(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("proj");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
    (dir, project)
}

fn scaffold_binary_with_math(name: &str) -> (tempfile::TempDir, PathBuf) {
    let (dir, project) = scaffold(name);
    fs::write(project.join("src/main.lis"), "fn main() {}\n").unwrap();
    fs::create_dir_all(project.join("src/math")).unwrap();
    fs::write(
        project.join("src/math/math.lis"),
        "pub fn add(a: int, b: int) -> int {\n  a + b\n}\n",
    )
    .unwrap();
    (dir, project)
}

#[test]
fn external_tests_run_and_report_with_internal_tests() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold_binary_with_math("extmix");
    fs::write(
        project.join("src/math/math.test.lis"),
        "#[test]\nfn internal_add() {\n  assert add(1, 1) == 2\n}\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("tests/integration")).unwrap();
    fs::write(
        project.join("tests/arithmetic.test.lis"),
        "import \"math\"\n\n#[test]\nfn adds_two() {\n  assert math.add(2, 2) == 4\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("tests/integration/flow.test.lis"),
        "import \"math\"\n\n#[test]\nfn end_to_end() {\n  assert math.add(20, 22) == 42\n}\n\n#[test]\nfn fails_on_purpose() {\n  assert math.add(2, 2) == 5\n}\n",
    )
    .unwrap();

    let test = lis(&project, &["test"]);
    let out = combined(&test);
    assert!(!test.status.success(), "expected failure: {out}");
    for group in [
        "\n  src/math/\n",
        "\n  tests/\n",
        "\n  tests/integration/\n",
    ] {
        assert!(out.contains(group), "missing group {group:?} in: {out}");
    }
    assert!(out.contains("1 failed · 3 passed"), "got: {out}");
    assert!(
        out.contains("✕ fails_on_purpose") && out.contains("flow.test.lis:"),
        "failure should name the test and frame its file: {out}"
    );
}

#[test]
fn build_never_emits_tests_and_cleans_leftovers() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold_binary_with_math("extclean");
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/arithmetic.test.lis"),
        "import \"math\"\n\n#[test]\nfn adds_two() {\n  assert math.add(2, 2) == 4\n}\n",
    )
    .unwrap();

    let test = lis(&project, &["test"]);
    assert!(test.status.success(), "test failed: {}", combined(&test));
    assert!(project.join("target/tests/arithmetic_test.go").exists());

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "build failed: {}", combined(&build));
    assert!(
        !project.join("target/tests").exists(),
        "build must remove the external test package"
    );
}

#[test]
fn external_test_cannot_reach_private_symbols() {
    let (_dir, project) = scaffold_binary_with_math("extvis");
    fs::write(
        project.join("src/math/math.lis"),
        "pub fn add(a: int, b: int) -> int {\n  a + b\n}\n\nfn secret() -> int {\n  41\n}\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/peek.test.lis"),
        "import \"math\"\n\n#[test]\nfn peeks() {\n  assert math.secret() == 41\n}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("not found in module"), "got: {out}");
}

#[test]
fn import_of_tests_namespace_is_rejected() {
    let (_dir, project) = scaffold_binary_with_math("extimport");
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/arithmetic.test.lis"),
        "import \"math\"\n\n#[test]\nfn adds_two() {\n  assert math.add(2, 2) == 4\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"tests\"\n\nfn main() {}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("reserved module name"), "got: {out}");
}

#[test]
fn src_tests_directory_is_reserved() {
    let (_dir, project) = scaffold_binary_with_math("extreserved");
    fs::create_dir_all(project.join("src/tests")).unwrap();
    fs::write(project.join("src/tests/helper.lis"), "pub fn x() {}\n").unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("Reserved module directory"), "got: {out}");
}

#[test]
fn non_test_file_under_tests_is_rejected() {
    let (_dir, project) = scaffold_binary_with_math("exthelper");
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(project.join("tests/helper.lis"), "pub fn shared() {}\n").unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("Non-test file under `tests/`"), "got: {out}");
}

#[test]
fn misnamed_test_file_under_tests_suggests_rename() {
    let (_dir, project) = scaffold_binary_with_math("extmisnamed");
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/arithmetic_test.lis"),
        "#[test]\nfn t() {}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("Misnamed test file"), "got: {out}");
    assert!(
        out.contains("tests/arithmetic.test.lis"),
        "should suggest the corrected name: {out}"
    );
}

#[test]
fn go_ignored_shape_under_tests_is_rejected() {
    let (_dir, project) = scaffold_binary_with_math("extshape");
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/api_windows.test.lis"),
        "#[test]\nfn t() {}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(
        out.contains("tests/api_windows.test.lis"),
        "the shape error should name the tests/ file: {out}"
    );
}

#[test]
fn library_project_runs_external_tests() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("example.com/acme/extlib");
    fs::create_dir_all(project.join("src/core")).unwrap();
    fs::write(
        project.join("src/core/core.lis"),
        "pub fn double(x: int) -> int {\n  x * 2\n}\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/api.test.lis"),
        "import \"core\"\n\n#[test]\nfn doubles() {\n  assert core.double(21) == 42\n}\n",
    )
    .unwrap();

    let test = lis(&project, &["test"]);
    let out = combined(&test);
    assert!(test.status.success(), "test failed: {out}");
    assert!(out.contains("\n  tests/\n"), "got: {out}");

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "build failed: {}", combined(&build));
    assert!(!project.join("target/tests").exists());
}
