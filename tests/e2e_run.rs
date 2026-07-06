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

fn lis(project: &Path, subcommand: &str) -> std::process::Output {
    let manifest = repo().join("Cargo.toml");
    Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", "lisette", "--", subcommand])
        .arg(project)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to invoke lisette")
}

const BOUND_MISMATCHED_BOX: &str = r#"
pub struct Box<T: Comparable> { pub items: Slice<T> }

impl<T: Ordered> Box<T> {
  pub fn equals(self, other: Box<T>) -> bool {
    self.items.equals(other.items)
  }
}
"#;

const WRAP_OVER_BOX: &str = r#"
import "box"

#[equality]
pub struct Wrap { pub box: box.Box<int> }
"#;

fn assert_rejected_at_check(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected a checker rejection:\nstdout: {stdout}\nstderr: {stderr}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("Cannot derive equality"),
        "expected `Cannot derive equality` at check, got:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !combined.contains("invalid operation"),
        "reached Go build instead of rejecting at check:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn parallel_equality_field_rejects_bound_mismatched_dependency() {
    if !go_available() {
        eprintln!(
            "skipping parallel_equality_field_rejects_bound_mismatched_dependency: `go` not found"
        );
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/box")).unwrap();
    fs::create_dir_all(project.join("src/wrap")).unwrap();
    for pad in ["a", "b", "c"] {
        fs::create_dir_all(project.join("src").join(pad)).unwrap();
        fs::write(
            project.join("src").join(pad).join(format!("{pad}.lis")),
            "pub fn ping() -> int { 1 }\n",
        )
        .unwrap();
    }
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/eqpar\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/box/box.lis"), BOUND_MISMATCHED_BOX).unwrap();
    fs::write(project.join("src/wrap/wrap.lis"), WRAP_OVER_BOX).unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "wrap"
import "box"
import "a"
import "b"
import "c"

fn main() {
  let _ = a.ping()
  let _ = b.ping()
  let _ = c.ping()
  let x = wrap.Wrap { box: box.Box { items: [1] } }
  let y = wrap.Wrap { box: box.Box { items: [1] } }
  let _ok = x.equals(y)
}
"#,
    )
    .unwrap();

    assert_rejected_at_check(&lis(&project, "check"));
}

#[test]
fn cached_equality_field_rejects_bound_mismatched_dependency() {
    if !go_available() {
        eprintln!(
            "skipping cached_equality_field_rejects_bound_mismatched_dependency: `go` not found"
        );
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/box")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/eqcache\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/box/box.lis"), BOUND_MISMATCHED_BOX).unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"box\"\n\nfn main() {\n  let _b = box.Box { items: [1] }\n}\n",
    )
    .unwrap();

    let first = lis(&project, "run");
    assert!(
        first.status.success(),
        "first run should cache `box`:\nstderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    fs::create_dir_all(project.join("src/wrap")).unwrap();
    fs::write(project.join("src/wrap/wrap.lis"), WRAP_OVER_BOX).unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "box"
import "wrap"

fn main() {
  let x = wrap.Wrap { box: box.Box { items: [1] } }
  let y = wrap.Wrap { box: box.Box { items: [1] } }
  let _ok = x.equals(y)
}
"#,
    )
    .unwrap();

    assert_rejected_at_check(&lis(&project, "check"));
}

#[test]
fn imported_weaker_interface_bound_equals_accepted() {
    if !go_available() {
        eprintln!("skipping imported_weaker_interface_bound_equals_accepted: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/iface")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/iface_bound\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/iface/iface.lis"),
        r#"pub interface Parent {
  fn p(self)
}

pub interface Child {
  embed Parent

  fn c(self)
}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "iface"

#[equality]
struct Box<T: iface.Child> { value: T }

impl<T: iface.Parent> Box<T> {
  fn equals(self, other: Box<T>) -> bool {
    true
  }
}

fn main() {}
"#,
    )
    .unwrap();

    let output = lis(&project, "check");
    assert!(
        output.status.success(),
        "imported weaker interface bound should be accepted:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_wrapper_for_function_named_t_builds_and_runs() {
    if !go_available() {
        eprintln!("skipping test_wrapper_for_function_named_t_builds_and_runs: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src")).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/tcollide\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.lis"), "fn main() {}\n").unwrap();
    fs::write(project.join("src/probe.test.lis"), "#[test]\nfn t() {}\n").unwrap();

    let output = lis(&project, "test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "a #[test] function named `t` must build and run:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !combined.contains("is not a function"),
        "the wrapper's *testing.T handle must not shadow `func t`:\nstdout: {stdout}\nstderr: {stderr}"
    );
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
fn run_unused_equality_type_keeps_nested_user_equals() {
    if !go_available() {
        eprintln!("skipping run_unused_equality_type_keeps_nested_user_equals: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    let invocation = scratch.path().join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"eqprune\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"

struct Inner { x: int }

fn same(a: int, b: int) -> bool { a == b }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    same(self.x, other.x)
  }
}

#[equality]
struct Outer { inner: Inner }

fn main() {
  fmt.Println("OK")
}
"#,
    )
    .unwrap();

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed (nested user equals or its helper likely pruned):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OK"),
        "program did not run:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn run_used_equality_dispatches_to_nested_equals_with_helper() {
    if !go_available() {
        eprintln!(
            "skipping run_used_equality_dispatches_to_nested_equals_with_helper: `go` not found"
        );
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    let invocation = scratch.path().join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"eqhelper\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"

struct Inner { x: int }

fn same(a: int, b: int) -> bool { a == b }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    same(self.x, other.x)
  }
}

#[equality]
struct Outer { inner: Inner }

fn main() {
  let a = Outer { inner: Inner { x: 1 } }
  let b = Outer { inner: Inner { x: 1 } }
  fmt.Println(a.equals(b))
}
"#,
    )
    .unwrap();

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("true"),
        "expected `true`:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn run_equality_on_recursive_enums_compares_structurally() {
    if !go_available() {
        eprintln!("skipping run_equality_on_recursive_enums_compares_structurally: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    let invocation = scratch.path().join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"eqrecursive\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"

#[equality]
enum List {
  Nil,
  Cons(int, List),
}

#[equality]
enum Tree {
  Leaf,
  Node(Pair),
}

#[equality]
struct Pair {
  l: Tree,
  r: Tree,
}

fn main() {
  let a = List.Cons(1, List.Cons(2, List.Nil))
  let b = List.Cons(1, List.Cons(2, List.Nil))
  let c = List.Cons(1, List.Cons(3, List.Nil))
  let d = List.Cons(1, List.Nil)
  fmt.Println(a.equals(b), a.equals(c), a.equals(d))
  let t1 = Tree.Node(Pair { l: Tree.Leaf, r: Tree.Leaf })
  let t2 = Tree.Node(Pair { l: Tree.Leaf, r: Tree.Leaf })
  let t3 = Tree.Node(Pair { l: t1, r: Tree.Leaf })
  fmt.Println(t1.equals(t2), t1.equals(t3))
}
"#,
    )
    .unwrap();

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.lines().any(|line| line == "true false false")
            && stdout.lines().any(|line| line == "true false"),
        "expected structural equality results:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn run_equality_matching_parametrized_interface_bound_builds() {
    if !go_available() {
        eprintln!(
            "skipping run_equality_matching_parametrized_interface_bound_builds: `go` not found"
        );
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    let invocation = scratch.path().join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"eqparam\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"

interface Parent<T> {
  fn p(self) -> T
}

struct Holder { tag: string }

impl Holder {
  fn p(self) -> string {
    self.tag
  }
}

#[equality]
struct Box<T: Parent<string>> { value: T }

impl<T: Parent<string>> Box<T> {
  fn equals(self, other: Box<T>) -> bool {
    self.value.p() == other.value.p()
  }
}

fn main() {
  let a = Box { value: Holder { tag: "x" } }
  let b = Box { value: Holder { tag: "y" } }
  fmt.Println(a.equals(b))
}
"#,
    )
    .unwrap();

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("false"),
        "expected `false`:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn run_equality_user_type_parametrized_bound_builds() {
    if !go_available() {
        eprintln!("skipping run_equality_user_type_parametrized_bound_builds: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    let invocation = scratch.path().join("invocation");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&invocation).unwrap();

    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"equserarg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "go:fmt"

struct Key { v: int }

interface Parent<T> {
  fn p(self) -> T
}

struct Leaf { k: Key }

impl Leaf {
  fn p(self) -> Key {
    self.k
  }
}

#[equality]
struct Box<T: Parent<Key>> { value: T }

impl<T: Parent<Key>> Box<T> {
  fn equals(self, other: Box<T>) -> bool {
    self.value.p().v == other.value.p().v
  }
}

fn main() {
  let a = Box { value: Leaf { k: Key { v: 1 } } }
  let b = Box { value: Leaf { k: Key { v: 2 } } }
  fmt.Println(a.equals(b))
}
"#,
    )
    .unwrap();

    let output = lis_run(&project, &invocation, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "lis run failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("false"),
        "expected `false`:\nstdout: {stdout}\nstderr: {stderr}"
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

#[test]
fn orphan_module_tests_are_discovered() {
    if !go_available() {
        eprintln!("skipping orphan_module_tests_are_discovered: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/orphan")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"orphandemo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.lis"), "fn main() {\n}\n").unwrap();
    fs::write(
        project.join("src/orphan/orphan.lis"),
        "pub fn helper() -> int { 42 }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/orphan/orphan.test.lis"),
        "#[test]\nfn orphan_pass() { assert helper() == 42 }\n\n#[test]\nfn orphan_fail() { assert helper() == 999 }\n",
    )
    .unwrap();

    let output = lis(&project, "test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        combined.contains("orphan_pass") && combined.contains("orphan_fail"),
        "tests in an unimported module must be discovered:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !output.status.success(),
        "the failing orphan test must make the run fail:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn single_file_check_ignores_unrelated_test_modules() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("standalone.lis"), "pub fn hi() -> int { 1 }\n").unwrap();
    fs::write(
        dir.join("sub/broken.test.lis"),
        "#[test]\nfn bad() { let _: int = \"type error\" }\n",
    )
    .unwrap();

    let output = lis(&dir.join("standalone.lis"), "check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "a standalone single-file check must not pull in an unrelated test module:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn loose_dir_check_does_not_duplicate_child_diagnostics() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::create_dir_all(dir.join("child")).unwrap();
    fs::write(dir.join("top.lis"), "pub fn top() -> int { 1 }\n").unwrap();
    fs::write(dir.join("child/child.lis"), "pub fn ch() -> int { 2 }\n").unwrap();
    fs::write(
        dir.join("child/child.test.lis"),
        "#[test]\nfn child_bad() { let _: int = \"err\" }\n",
    )
    .unwrap();

    let output = lis(dir, "check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let hits = combined.matches("child_bad").count();
    assert_eq!(
        hits, 1,
        "a loose-directory check must report a child module's diagnostic once, not once per ancestor sweep:\n{combined}"
    );
}

#[test]
fn t_log_renders_logged_values_in_a_logs_section() {
    if !go_available() {
        eprintln!("skipping t_log_renders_logged_values_in_a_logs_section: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"logdemo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.lis"), "fn main() {\n}\n").unwrap();
    fs::write(
        project.join("src/demo.test.lis"),
        "#[test]\nfn logs_a_value(t: TestContext) {\n  let user = \"alice\"\n  t.log(user)\n  assert user.length() == 5\n}\n",
    )
    .unwrap();

    let output = lis(&project, "test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "lis test should pass with a logged value:\n{combined}"
    );
    assert!(
        combined.contains("Logs") && combined.contains("\"alice\""),
        "the report should show the logged value in a Logs section:\n{combined}"
    );
}
