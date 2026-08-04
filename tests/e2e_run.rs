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
fn growing_circular_alias_rejected_at_check() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"growingalias\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        r#"type A<T> = Option<A<Option<T>>>
type M = Map<A<int>, int>

fn main() {}
"#,
    )
    .unwrap();

    assert_rejected_at_check(&lis(&project, "check"), "Circular type alias");
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
    fs::write(dir.join("script.lis"), "pub fn hi() -> int { 1 }\n").unwrap();
    fs::write(
        dir.join("sub/broken.test.lis"),
        "#[test]\nfn bad() { let _: int = \"type error\" }\n",
    )
    .unwrap();

    let output = lis(&dir.join("script.lis"), "check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "a single-file script check must not pull in an unrelated test module:\nstdout: {stdout}\nstderr: {stderr}"
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

#[test]
fn check_and_run_agree_on_a_named_file() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(
        dir.join("main.lis"),
        "import \"go:fmt\"\n\nfn main() {\n  fmt.Println(greet())\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("greet.lis"),
        "fn greet() -> string {\n  \"hi\"\n}\n",
    )
    .unwrap();

    let check_file = lis(&dir.join("main.lis"), "check");
    let run_file = lis(&dir.join("main.lis"), "run");
    let check_dir = lis(dir, "check");

    for (label, output) in [
        ("check <file>", &check_file),
        ("run <file>", &run_file),
        ("check <dir>", &check_dir),
    ] {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.status.success(), "{label} passed:\n{combined}");
        assert!(
            combined.contains("Name not found"),
            "{label} missed the sibling call:\n{combined}"
        );
    }
}

#[test]
fn a_directory_of_single_file_programs_checks_clean() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    for (name, text) in [("a.lis", "script a"), ("b.lis", "script b")] {
        fs::write(
            dir.join(name),
            format!("import \"go:fmt\"\n\nfn main() {{\n  fmt.Println(\"{text}\")\n}}\n"),
        )
        .unwrap();
    }

    let output = lis(dir, "check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "unrelated single-file programs must not collide:\n{combined}"
    );

    if !go_available() {
        eprintln!("skipping the run half of a_directory_of_single_file_programs_checks_clean");
        return;
    }
    for (name, text) in [("a.lis", "script a"), ("b.lis", "script b")] {
        let output = lis(&dir.join(name), "run");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), text);
    }
}

fn scaffold_util_project(root: &Path) -> PathBuf {
    let project = root.join("proj");
    fs::create_dir_all(project.join("src/util")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"demoproj\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"go:fmt\"\nimport \"util\"\n\nfn main() {\n  fmt.Println(util.greet())\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("src/util/util.lis"),
        "pub fn greet() -> string {\n  \"hi from util\"\n}\n",
    )
    .unwrap();
    project
}

#[test]
fn a_project_source_file_is_checked_as_its_project() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_util_project(scratch.path());

    let output = lis(&project.join("src/main.lis"), "check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.status.success(),
        "a project entrypoint must resolve its own modules:\n{combined}"
    );
    assert!(
        combined.contains("2 files"),
        "the whole project should have been checked:\n{combined}"
    );
    assert!(
        !combined.contains("lis new"),
        "a file inside a project must never be told to create one:\n{combined}"
    );
}

#[test]
fn running_a_project_module_names_the_project() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_util_project(scratch.path());

    let output = lis(&project.join("src/util/util.lis"), "run");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!output.status.success(), "a module is not a program");
    assert!(combined.contains("demoproj"), "{combined}");
    assert!(combined.contains("main"), "{combined}");
    assert!(!combined.contains("lis new"), "{combined}");
}

#[test]
fn a_file_beside_a_project_is_not_told_to_create_one() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_util_project(scratch.path());
    fs::write(
        project.join("play.lis"),
        "import \"util\"\n\nfn main() {\n  let _ = util.greet()\n}\n",
    )
    .unwrap();

    let output = lis(&project.join("play.lis"), "check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!output.status.success(), "a lone file resolves no modules");
    assert!(combined.contains("Module not found"), "{combined}");
    assert!(!combined.contains("lis new"), "{combined}");
}

#[test]
fn a_tree_walk_checks_a_nested_project_as_a_project() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_util_project(scratch.path());
    fs::write(
        scratch.path().join("loose.lis"),
        "import \"go:fmt\"\n\nfn main() {\n  fmt.Println(\"loose\")\n}\n",
    )
    .unwrap();

    for target in [scratch.path(), &project.join("src")] {
        let output = lis(target, "check");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success(),
            "project files under {} must resolve their own modules:\n{combined}",
            target.display()
        );
    }
}

#[test]
fn a_non_lisette_file_under_src_is_not_a_project_target() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scaffold_util_project(scratch.path());
    fs::write(project.join("src/README.md"), "not lisette\n").unwrap();

    let output = lis(&project.join("src/README.md"), "build");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "naming a non-`.lis` file must not build its directory's project:\n{combined}"
    );
    assert!(
        !combined.contains("Emit completed"),
        "the project must not be built:\n{combined}"
    );
}

#[test]
fn a_tree_walk_reports_a_nested_project_with_no_sources() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("decl");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"declonly\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/api.d.lis"), "pub fn helper() -> int\n").unwrap();

    let walked = lis(scratch.path(), "check");
    let direct = lis(&project, "check");

    for (label, output) in [("tree walk", &walked), ("direct", &direct)] {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success(),
            "{label} must not pass a project with no production sources:\n{combined}"
        );
        assert!(
            combined.contains("No Lisette sources"),
            "{label}: {combined}"
        );
    }
}

fn lis_in(cwd: &Path, args: &[&str]) -> std::process::Output {
    let manifest = repo().join("Cargo.toml");
    Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", "lisette", "--"])
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to invoke lisette")
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const GREETER: &str = "import \"go:fmt\"\n\nfn main() {\n  fmt.Println(\"hi\")\n}\n";

#[test]
fn emit_writes_one_go_file_beside_no_target_dir() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();

    let output = lis_in(dir, &["emit", "greet.lis"]);

    assert!(output.status.success(), "{}", combined(&output));
    let go = fs::read_to_string(dir.join("greet.go")).expect("greet.go must exist");
    assert!(go.contains("package main"), "{go}");
    assert!(
        !dir.join("target").exists(),
        "a script emit must not create `target/`"
    );
}

#[test]
fn emit_output_flag_chooses_the_path() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();

    let output = lis_in(dir, &["emit", "greet.lis", "-o", "out/custom.go"]);

    assert!(output.status.success(), "{}", combined(&output));
    assert!(dir.join("out/custom.go").is_file());
    assert!(!dir.join("greet.go").exists());
}

#[test]
fn build_links_a_runnable_binary_into_the_working_dir() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();

    let output = lis_in(dir, &["build", "greet.lis"]);
    assert!(output.status.success(), "{}", combined(&output));
    assert!(
        !dir.join("target").exists(),
        "a script build must not create `target/`"
    );

    let ran = Command::new(dir.join("greet"))
        .output()
        .expect("the built binary must run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "hi");
}

#[test]
fn an_extensionless_script_runs_checks_and_builds() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("backup"), GREETER).unwrap();

    let checked = lis_in(dir, &["check", "backup"]);
    assert!(checked.status.success(), "{}", combined(&checked));

    let ran = lis_in(dir, &["run", "backup"]);
    assert!(ran.status.success(), "{}", combined(&ran));
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "hi");

    let built = lis_in(dir, &["build", "backup", "-o", "backup.bin"]);
    assert!(built.status.success(), "{}", combined(&built));
    assert!(dir.join("backup.bin").is_file());
}

#[test]
fn build_refuses_to_overwrite_the_script_it_compiles() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("backup"), GREETER).unwrap();

    let output = lis_in(dir, &["build", "backup"]);

    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("is the script being compiled"),
        "{}",
        combined(&output)
    );
    assert_eq!(
        fs::read_to_string(dir.join("backup")).unwrap(),
        GREETER,
        "the source must survive a refused build"
    );
}

#[test]
fn output_flag_aimed_at_any_input_file_is_refused() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();

    for args in [
        vec!["emit", "greet.lis", "-o", "greet.lis"],
        vec!["build", "greet.lis", "-o", "greet.lis"],
    ] {
        let output = lis_in(dir, &args);
        assert!(!output.status.success(), "{args:?}: {}", combined(&output));
        assert_eq!(
            fs::read_to_string(dir.join("greet.lis")).unwrap(),
            GREETER,
            "{args:?} must leave the source untouched"
        );
    }
}

#[test]
fn a_script_named_the_way_go_ignores_still_builds() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();

    for name in ["_leading.lis", ".hidden.lis", "-dashed.lis", "..."] {
        fs::write(dir.join(name), GREETER).unwrap();
        let relative = format!("./{name}");

        let ran = lis_in(dir, &["run", &relative]);
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout).trim(),
            "hi",
            "`{name}`: {}",
            combined(&ran)
        );

        let built = lis_in(dir, &["build", &relative, "-o", "out.bin"]);
        assert!(built.status.success(), "`{name}`: {}", combined(&built));
    }
}

#[test]
fn a_stale_go_file_in_the_build_directory_is_pruned() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();

    let scratch_tmp = dir.join("tmp");
    fs::create_dir(&scratch_tmp).unwrap();
    let build = || {
        let manifest = repo().join("Cargo.toml");
        Command::new("cargo")
            .args(["run", "--quiet", "--manifest-path"])
            .arg(&manifest)
            .args(["-p", "lisette", "--", "build", "greet.lis", "-o", "out.bin"])
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .env("TMPDIR", &scratch_tmp)
            .env("TMP", &scratch_tmp)
            .env("TEMP", &scratch_tmp)
            .output()
            .expect("failed to invoke lisette")
    };

    let first = build();
    assert!(first.status.success(), "{}", combined(&first));

    let build_dir = fs::read_dir(&scratch_tmp)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.join("greet.go").is_file())
        .expect("the build directory must sit under the test's own TMPDIR");
    fs::write(
        build_dir.join("orphan.go"),
        "package main\n\nfunc dupe() {}\n",
    )
    .unwrap();

    let second = build();
    assert!(second.status.success(), "{}", combined(&second));
    assert!(
        !build_dir.join("orphan.go").exists(),
        "a Go file outside the emit must not survive into `go build`"
    );
}

#[test]
fn a_hard_link_to_the_script_is_refused() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();
    fs::hard_link(dir.join("greet.lis"), dir.join("linked.go")).unwrap();

    let output = lis_in(dir, &["emit", "greet.lis", "-o", "linked.go"]);

    assert!(!output.status.success(), "{}", combined(&output));
    assert_eq!(
        fs::read_to_string(dir.join("greet.lis")).unwrap(),
        GREETER,
        "a hard link shares no pathname with its source, and still is it"
    );
}

#[test]
fn a_path_through_a_missing_directory_cannot_reach_the_script() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();

    for args in [
        vec!["emit", "greet.lis", "-o", "missing/../greet.lis"],
        vec!["build", "greet.lis", "-o", "missing/../greet.lis"],
    ] {
        let output = lis_in(dir, &args);
        assert!(!output.status.success(), "{args:?}: {}", combined(&output));
        assert_eq!(
            fs::read_to_string(dir.join("greet.lis")).unwrap(),
            GREETER,
            "{args:?} must not reach the source through a directory made later"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_parent_cannot_reach_the_script() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::create_dir(dir.join("real")).unwrap();
    std::os::unix::fs::symlink("real", dir.join("link")).unwrap();
    fs::write(dir.join("real/greet.lis"), GREETER).unwrap();

    for args in [
        vec!["emit", "greet.lis", "-o", "../link/missing/../greet.lis"],
        vec!["build", "greet.lis", "-o", "../link/missing/../greet.lis"],
    ] {
        let output = lis_in(&dir.join("real"), &args);
        assert!(!output.status.success(), "{args:?}: {}", combined(&output));
        assert_eq!(
            fs::read_to_string(dir.join("real/greet.lis")).unwrap(),
            GREETER,
            "{args:?} must not reach the source by crossing a symlink with `..`"
        );
    }
}

#[test]
fn a_path_that_can_only_name_a_directory_is_refused() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();
    fs::write(dir.join("victim"), "precious\n").unwrap();

    for args in [
        vec!["emit", "greet.lis", "-o", "fresh/"],
        vec!["emit", "greet.lis", "-o", "victim/"],
        vec!["build", "greet.lis", "-o", "fresh/"],
        vec!["emit", "greet.lis", "-o", "fresh/."],
        vec!["emit", "greet.lis", "-o", "victim/."],
        vec!["build", "greet.lis", "-o", "fresh/."],
    ] {
        let output = lis_in(dir, &args);
        let text = combined(&output);
        assert!(!output.status.success(), "{args:?}: {text}");
        assert!(text.contains("names a directory"), "{args:?}: {text}");
    }

    assert!(
        !dir.join("fresh").exists(),
        "these paths name a directory, so no file `fresh` may appear"
    );
    assert_eq!(
        fs::read_to_string(dir.join("victim")).unwrap(),
        "precious\n",
        "these are paths the kernel refuses, so the file must survive"
    );

    let ordinary = lis_in(dir, &["emit", "greet.lis", "-o", "./out.go"]);
    assert!(ordinary.status.success(), "{}", combined(&ordinary));
    assert!(
        dir.join("out.go").is_file(),
        "a `.` inside a path is still an ordinary path"
    );
}

#[test]
fn a_path_leading_through_a_file_is_refused() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();
    fs::write(dir.join("blocker"), "not a directory\n").unwrap();

    for args in [
        vec!["emit", "greet.lis", "-o", "blocker/../out.go"],
        vec!["build", "greet.lis", "-o", "blocker/../out"],
    ] {
        let output = lis_in(dir, &args);
        let text = combined(&output);
        assert!(!output.status.success(), "{args:?}: {text}");
        assert!(text.contains("is not a directory"), "{args:?}: {text}");
    }

    assert!(
        !dir.join("out.go").exists() && !dir.join("out").exists(),
        "a path the kernel answers with ENOTDIR must not be folded into a sibling write"
    );
}

#[test]
fn a_directory_destination_is_refused_rather_than_written_into() {
    if !go_available() {
        return;
    }
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();
    fs::write(dir.join("greet.lis"), GREETER).unwrap();
    fs::create_dir(dir.join("greet")).unwrap();
    fs::create_dir(dir.join("dest")).unwrap();

    for args in [
        vec!["build", "greet.lis"],
        vec!["build", "greet.lis", "-o", "dest"],
        vec!["emit", "greet.lis", "-o", "dest"],
    ] {
        let output = lis_in(dir, &args);
        let text = combined(&output);
        assert!(!output.status.success(), "{args:?}: {text}");
        assert!(text.contains("is a directory"), "{args:?}: {text}");
    }

    assert!(
        fs::read_dir(dir.join("greet")).unwrap().next().is_none(),
        "nothing may be written inside the colliding directory"
    );
    assert!(
        fs::read_dir(dir.join("dest")).unwrap().next().is_none(),
        "nothing may be written inside the named directory"
    );
}

#[test]
fn a_nonexistent_target_names_the_path() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let dir = scratch.path();

    for subcommand in ["run", "emit", "build"] {
        let output = lis_in(dir, &[subcommand, "nope"]);
        let text = combined(&output);
        assert!(!output.status.success(), "{subcommand}: {text}");
        assert!(
            text.contains("Path `nope` does not exist"),
            "{subcommand}: {text}"
        );
    }
}

#[test]
fn project_emit_and_build_reject_the_output_flag() {
    let scratch = tempfile::tempdir().expect("create temp dir");
    let project = scratch.path().join("proj");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"outflag\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.lis"), "fn main() {}\n").unwrap();

    for subcommand in ["emit", "build"] {
        let output = lis_in(&project, &[subcommand, ".", "-o", "somewhere"]);
        let text = combined(&output);
        assert!(!output.status.success(), "{subcommand}: {text}");
        assert!(
            text.contains("has no meaning for a project"),
            "{subcommand}: {text}"
        );
    }
}
