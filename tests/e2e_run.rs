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

const STRENGTHENED_IMPL_BOX: &str = r#"
pub struct Box<T: Comparable> { pub items: Slice<T> }

impl<T: Ordered> Box<T> {
  pub fn equals(self, other: Box<T>) -> bool {
    self.items.equals(other.items)
  }
}
"#;

const CONSTRAINED_MAP_BOX: &str = r#"
pub struct Box<T: Comparable> { pub values: Map<T, int> }
"#;

const UNBOUNDED_WRAP: &str = r#"
import "box"

pub struct Wrap<T> { pub value: box.Box<T> }
"#;

fn assert_rejected_at_check(output: &std::process::Output, expected: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected a checker rejection:\nstdout: {stdout}\nstderr: {stderr}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains(expected),
        "expected `{expected}` at check, got:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !combined.contains("invalid operation"),
        "reached Go build instead of rejecting at check:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn parallel_strengthened_impl_bound_rejected() {
    if !go_available() {
        eprintln!("skipping parallel_strengthened_impl_bound_rejected: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/box")).unwrap();
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
    fs::write(project.join("src/box/box.lis"), STRENGTHENED_IMPL_BOX).unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "box"
import "a"
import "b"
import "c"

fn main() {
  let _ = a.ping()
  let _ = b.ping()
  let _ = c.ping()
  let _ = box.Box { items: [1] }
}
"#,
    )
    .unwrap();

    assert_rejected_at_check(
        &lis(&project, "check"),
        "`impl` cannot strengthen receiver bounds",
    );
}

#[test]
fn cached_constrained_type_rejects_unbounded_argument() {
    if !go_available() {
        eprintln!("skipping cached_constrained_type_rejects_unbounded_argument: `go` not found");
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
    fs::write(project.join("src/box/box.lis"), CONSTRAINED_MAP_BOX).unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"box\"\n\nfn main() {\n  let _ = box.Box { values: Map.new<int, int>() }\n}\n",
    )
    .unwrap();

    let first = lis(&project, "run");
    assert!(
        first.status.success(),
        "first run should cache `box`:\nstderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    fs::create_dir_all(project.join("src/wrap")).unwrap();
    fs::write(project.join("src/wrap/wrap.lis"), UNBOUNDED_WRAP).unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"import "wrap"

fn main() {
  let _ = 1
}
"#,
    )
    .unwrap();

    assert_rejected_at_check(&lis(&project, "check"), "Missing bound on type parameter");
}

#[test]
fn cached_aliased_interface_bound_keeps_its_identity() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/constraints")).unwrap();
    fs::create_dir_all(project.join("src/box")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/boundcache\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/constraints/constraints.lis"),
        "pub interface Parent<T> {\n  fn value() -> T\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("src/box/box.lis"),
        "import c \"constraints\"\n\npub interface Box<T: c.Parent<string>> {}\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"box\"\n\nfn main() {}\n",
    )
    .unwrap();

    let first = lis(&project, "check");
    assert!(
        first.status.success(),
        "first check should cache `box`:\nstderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    fs::create_dir_all(project.join("src/wrap")).unwrap();
    fs::write(
        project.join("src/wrap/wrap.lis"),
        r#"import "box"
import c "constraints"

pub interface Wrap<U: box.Box<T>, T: c.Parent<string>> {}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"wrap\"\n\nfn main() {}\n",
    )
    .unwrap();

    let matching = lis(&project, "check");
    assert!(
        matching.status.success(),
        "matching imported bounds should pass after a cache hit:\nstderr: {}",
        String::from_utf8_lossy(&matching.stderr)
    );

    fs::write(
        project.join("src/wrap/wrap.lis"),
        r#"import "box"
import c "constraints"

pub interface Wrap<U: box.Box<T>, T: c.Parent<int>> {}
"#,
    )
    .unwrap();
    assert_rejected_at_check(&lis(&project, "check"), "Missing bound on type parameter");
}

#[test]
fn cached_public_bound_can_reference_private_interface() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/box")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/privatebound\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/box/box.lis"),
        r#"interface Hidden {
  fn show() -> string
}

pub interface Box<T: Hidden> {}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"box\"\n\nfn main() {}\n",
    )
    .unwrap();

    let first = lis(&project, "check");
    assert!(
        first.status.success(),
        "first check should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    let second = lis(&project, "check");
    assert!(
        second.status.success(),
        "cached check should succeed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    fs::create_dir_all(project.join("src/wrap")).unwrap();
    fs::write(
        project.join("src/wrap/wrap.lis"),
        r#"import "box"

struct Plain {}

pub interface Wrap<T: box.Box<Plain>> {}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"wrap\"\n\nfn main() {}\n",
    )
    .unwrap();
    assert_rejected_at_check(&lis(&project, "check"), "Interface not implemented");
}

#[test]
fn cached_public_function_bound_can_reference_private_interface() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/api")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/privatefunctionbound\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/api/api.lis"),
        r#"interface Hidden {
  fn show() -> string
}

pub fn use<T: Hidden>(_value: T) {}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"api\"\n\nfn main() {}\n",
    )
    .unwrap();

    let first = lis(&project, "check");
    assert!(
        first.status.success(),
        "first check should cache `api`:\nstderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    fs::create_dir_all(project.join("src/use_api")).unwrap();
    fs::write(
        project.join("src/use_api/use_api.lis"),
        r#"import "api"

struct Plain {}

pub fn call() {
  api.use(Plain {})
}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"use_api\"\n\nfn main() {}\n",
    )
    .unwrap();

    assert_rejected_at_check(&lis(&project, "check"), "Interface not implemented");
}

#[test]
fn cached_public_interface_can_embed_private_parent() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/api")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/privateparent\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/api/api.lis"),
        r#"interface Hidden {
  fn show() -> string
}

pub interface Public {
  embed Hidden
}

pub fn use(_value: Public) {}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"api\"\n\nfn main() {}\n",
    )
    .unwrap();

    let first = lis(&project, "check");
    assert!(
        first.status.success(),
        "first check should cache `api`:\nstderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    fs::create_dir_all(project.join("src/use_api")).unwrap();
    fs::write(
        project.join("src/use_api/use_api.lis"),
        r#"import "api"

struct Plain {}

pub fn call() {
  api.use(Plain {})
}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"use_api\"\n\nfn main() {}\n",
    )
    .unwrap();

    assert_rejected_at_check(&lis(&project, "check"), "Interface not implemented");
}

#[test]
fn parallel_registration_validates_bounds_with_dependency_ufcs_methods() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    for module in ["dep", "use_a", "use_b"] {
        fs::create_dir_all(project.join("src").join(module)).unwrap();
    }
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/parallelbounds\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/dep/dep.lis"),
        r#"pub struct Box<T> {
  pub value: T
}

impl Box<int> {
  pub fn show(self) -> string { "box" }
}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/use_a/use_a.lis"),
        r#"import "dep"

pub interface Shower {
  fn show() -> string
}

pub interface Need<T: Shower> {}

pub interface Uses<T: Need<dep.Box<int>>> {}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/use_b/use_b.lis"),
        r#"import "dep"

pub struct Keep {
  pub value: dep.Box<int>
}
"#,
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"use_a\"\nimport \"use_b\"\n\nfn main() {}\n",
    )
    .unwrap();

    assert_rejected_at_check(
        &lis(&project, "check"),
        "Specialized impl cannot satisfy interface",
    );
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
  fn p()
}

pub interface Child {
  embed Parent

  fn c()
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
  fn p() -> T
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
  fn p() -> T
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

fn scaffold_orphan_project(root: &Path, orphan_body: &str) -> PathBuf {
    let project = root.join("proj");
    fs::create_dir_all(project.join("src/lib")).unwrap();
    fs::create_dir_all(project.join("src/orphan")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/orphanproj\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/lib/lib.lis"), "pub fn f() -> int { 1 }\n").unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"lib\"\n\nfn main() {\n  let _ = lib.f()\n}\n",
    )
    .unwrap();
    fs::write(project.join("src/orphan/orphan.lis"), orphan_body).unwrap();
    project
}

fn contains_file_named(dir: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if contains_file_named(&path, name) {
                return true;
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return true;
        }
    }
    false
}

#[test]
fn broken_orphan_module_fails_check() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_orphan_project(
        scratch.path(),
        "pub fn broken(x: int) -> int {\n  x + \"boom\"\n}\n",
    );

    let output = lis(&project, "check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !output.status.success(),
        "a type error in an unimported module must fail check:\n{combined}"
    );
    assert!(
        combined.contains("Type mismatch") && combined.contains("infer.type_mismatch"),
        "the orphan's real type error should surface:\n{combined}"
    );
}

#[test]
fn clean_orphan_module_warns_at_check_but_passes() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_orphan_project(scratch.path(), "pub fn helper() -> int { 1 }\n");

    let output = lis(&project, "check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "a clean unreachable module is a warning, not an error:\n{combined}"
    );
    assert!(
        combined.contains("Unreachable module: `orphan`"),
        "check should warn about the unreachable module:\n{combined}"
    );
}

#[test]
fn build_excludes_and_notes_orphan_module() {
    if !go_available() {
        eprintln!("skipping build_excludes_and_notes_orphan_module: `go` not found");
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_orphan_project(scratch.path(), "pub fn helper() -> int { 1 }\n");

    let output = lis(&project, "build");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "the binary is sound, so build succeeds:\n{combined}"
    );
    assert!(
        combined.contains("Unreachable module: `orphan`"),
        "build should warn about the unreachable module:\n{combined}"
    );
    assert!(
        !contains_file_named(&project.join("target"), "orphan.go"),
        "the orphan module must not be emitted into target/"
    );
}

fn target_contains_text(dir: &Path, needle: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if target_contains_text(&path, needle) {
                return true;
            }
        } else if let Ok(content) = fs::read_to_string(&path)
            && content.contains(needle)
        {
            return true;
        }
    }
    false
}

#[test]
fn lis_test_does_not_emit_production_orphan_or_its_dependency() {
    if !go_available() {
        eprintln!(
            "skipping lis_test_does_not_emit_production_orphan_or_its_dependency: `go` not found"
        );
        return;
    }

    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src/lib")).unwrap();
    fs::create_dir_all(project.join("src/orphan")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/user/orphantest\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/lib/lib.lis"), "pub fn f() -> int { 1 }\n").unwrap();
    fs::write(
        project.join("src/lib/lib.test.lis"),
        "#[test]\nfn f_returns_one() {\n  assert f() == 1\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"lib\"\n\nfn main() {\n  let _ = lib.f()\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("src/orphan/orphan.lis"),
        "import \"go:archive/tar\"\n\npub fn make() -> tar.Header {\n  tar.Header {}\n}\n",
    )
    .unwrap();

    let output = lis(&project, "test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(output.status.success(), "lis test should pass:\n{combined}");
    assert!(
        !contains_file_named(&project.join("target"), "orphan.go"),
        "lis test must not emit the production orphan into target/"
    );
    assert!(
        !target_contains_text(&project.join("target"), "archive/tar"),
        "the orphan's unique Go dependency must not leak into emitted output:\n{combined}"
    );
}
