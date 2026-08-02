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
fn loose_directory_tests_import_is_not_reserved_but_unresolved() {
    let dir = tempfile::tempdir().unwrap();
    let loose = dir.path();
    fs::write(loose.join("a.lis"), "import \"tests\"\n\nfn f() { g() }\n").unwrap();
    fs::write(loose.join("b.lis"), "pub fn g() {}\n").unwrap();

    let check = lis(loose, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(
        out.contains("Module not found"),
        "a missing `tests` module outside a project is unresolved, not reserved: {out}"
    );
    assert!(
        !out.contains("reserved"),
        "the `tests` reservation must not apply outside a project: {out}"
    );
}

#[test]
fn loose_directory_tests_folder_is_unresolved_not_reserved() {
    let dir = tempfile::tempdir().unwrap();
    let loose = dir.path();
    fs::write(
        loose.join("a.lis"),
        "import \"tests\"\n\nfn f() { tests.helper() }\n",
    )
    .unwrap();
    fs::create_dir_all(loose.join("tests")).unwrap();
    fs::write(loose.join("tests/util.lis"), "pub fn helper() {}\n").unwrap();

    let check = lis(loose, &["check"]);
    let out = combined(&check);
    assert!(
        !check.status.success(),
        "a lone file resolves no sibling module, `tests/` included: {out}"
    );
    assert!(out.contains("Module not found"), "got: {out}");
    assert!(
        !out.contains("reserved"),
        "the `tests` reservation must not apply outside a project: {out}"
    );
}

#[test]
fn subpackage_only_library_rejects_root_import() {
    let (_dir, project) = scaffold("example.com/you/geo");
    fs::create_dir_all(project.join("src/shapes")).unwrap();
    fs::write(
        project.join("src/shapes/shapes.lis"),
        "pub fn area() -> int { 4 }\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/api.test.lis"),
        "import \"root\"\n\n#[test]\nfn t() {\n  assert root.missing() == 1\n}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("no root package"), "got: {out}");
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

fn scaffold_geo_library() -> (tempfile::TempDir, PathBuf) {
    let (dir, project) = scaffold("example.com/acme/geo");
    fs::write(
        project.join("src/geo.lis"),
        "pub fn distance(a: int, b: int) -> int {\n  if a > b { a - b } else { b - a }\n}\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("tests")).unwrap();
    (dir, project)
}

#[test]
fn warm_check_catches_root_api_change() {
    let (_dir, project) = scaffold_geo_library();
    fs::write(
        project.join("tests/api.test.lis"),
        "import \"root\"\n\n#[test]\nfn value() {\n  assert root.distance(2, 9) == 7\n}\n",
    )
    .unwrap();

    let cold = lis(&project, &["check"]);
    assert!(
        cold.status.success(),
        "cold check failed: {}",
        combined(&cold)
    );

    fs::write(
        project.join("src/geo.lis"),
        "pub fn renamed(a: int, b: int) -> int {\n  if a > b { a - b } else { b - a }\n}\n",
    )
    .unwrap();

    let warm = lis(&project, &["check"]);
    let out = combined(&warm);
    assert!(
        !warm.status.success(),
        "warm check missed the root change: {out}"
    );
    assert!(
        out.contains("not found in module"),
        "the stale root member must be reported on a warm run: {out}"
    );
}

#[test]
fn library_external_test_imports_root() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold_geo_library();
    fs::write(
        project.join("tests/api.test.lis"),
        "import \"root\"\n\n#[test]\nfn symmetric() {\n  assert root.distance(2, 9) == root.distance(9, 2)\n}\n\n#[test]\nfn value() {\n  assert root.distance(2, 9) == 7\n}\n",
    )
    .unwrap();

    let test = lis(&project, &["test"]);
    let out = combined(&test);
    assert!(test.status.success(), "test failed: {out}");
    assert!(out.contains("\n  tests/\n"), "got: {out}");
    assert!(out.contains("2 passed"), "got: {out}");

    let emitted = fs::read_to_string(project.join("target/tests/api_test.go")).unwrap();
    assert!(
        emitted.contains("\"example.com/acme/geo\""),
        "root package imported at the bare module path: {emitted}"
    );
    assert!(
        !emitted.contains("_entry_"),
        "the internal entry id must not leak into Go: {emitted}"
    );
    assert!(
        emitted.contains("root.Distance("),
        "root API referenced through the `root` qualifier: {emitted}"
    );

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "build failed: {}", combined(&build));
}

#[test]
fn aliased_root_import_binds() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold_geo_library();
    fs::write(
        project.join("tests/api.test.lis"),
        "import g \"root\"\n\n#[test]\nfn value() {\n  assert g.distance(2, 9) == 7\n}\n",
    )
    .unwrap();

    let test = lis(&project, &["test"]);
    assert!(test.status.success(), "test failed: {}", combined(&test));
    let emitted = fs::read_to_string(project.join("target/tests/api_test.go")).unwrap();
    assert!(
        emitted.contains("g.Distance("),
        "the source alias carries through to the Go qualifier: {emitted}"
    );
}

#[test]
fn root_import_from_src_is_rejected() {
    let (_dir, project) = scaffold("example.com/acme/geo");
    fs::write(project.join("src/geo.lis"), "pub fn top() -> int { 1 }\n").unwrap();
    fs::create_dir_all(project.join("src/util")).unwrap();
    fs::write(
        project.join("src/util/util.lis"),
        "import \"root\"\npub fn helper() -> int {\n  root.top()\n}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(
        out.contains("only from external tests"),
        "src imports of root are steered away: {out}"
    );
}

#[test]
fn root_import_in_binary_is_rejected() {
    let (_dir, project) = scaffold("example.com/acme/bin");
    fs::write(project.join("src/main.lis"), "fn main() {}\n").unwrap();
    fs::create_dir_all(project.join("tests")).unwrap();
    fs::write(
        project.join("tests/api.test.lis"),
        "import \"root\"\n\n#[test]\nfn t() {\n  assert root.x() == 1\n}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(
        out.contains("A binary has no importable root"),
        "binary roots are not importable: {out}"
    );
}

#[test]
fn src_root_directory_is_reserved() {
    let (_dir, project) = scaffold("example.com/acme/geo");
    fs::write(project.join("src/geo.lis"), "pub fn x() -> int { 1 }\n").unwrap();
    fs::create_dir_all(project.join("src/root")).unwrap();
    fs::write(project.join("src/root/r.lis"), "pub fn y() -> int { 2 }\n").unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("Reserved module directory"), "got: {out}");
}

#[test]
fn src_entry_directory_is_reserved() {
    let (_dir, project) = scaffold("example.com/acme/geo");
    fs::write(project.join("src/geo.lis"), "pub fn x() -> int { 1 }\n").unwrap();
    fs::create_dir_all(project.join("src/_entry_")).unwrap();
    fs::write(
        project.join("src/_entry_/e.lis"),
        "pub fn y() -> int { 2 }\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(out.contains("Reserved module directory"), "got: {out}");
    assert!(
        out.contains("internal entry module"),
        "the entry collision must be named: {out}"
    );
}

#[test]
fn type_mismatch_through_root_shows_source_spelling() {
    let (_dir, project) = scaffold_geo_library();
    fs::write(
        project.join("tests/api.test.lis"),
        "import \"root\"\n\n#[test]\nfn t() {\n  let x: int = root\n  assert x == 1\n}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    let out = combined(&check);
    assert!(!check.status.success(), "expected failure: {out}");
    assert!(
        out.contains("found `root`"),
        "the namespace type must render as `root`, not the internal id: {out}"
    );
    assert!(
        !out.contains("_entry_"),
        "the internal id must not leak: {out}"
    );
}
