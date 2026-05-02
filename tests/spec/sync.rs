use std::path::Path;
use std::process::Command;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn run_sync(project_dir: &Path) -> std::process::Output {
    let manifest_path = repo_root().join("Cargo.toml");
    Command::new("cargo")
        .args(["run", "--quiet", "-p", "lisette", "--manifest-path"])
        .arg(&manifest_path)
        .args(["--", "sync"])
        .current_dir(project_dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lis sync")
}

fn write_project(dir: &Path, manifest: &str, sources: &[(&str, &str)]) {
    std::fs::write(dir.join("lisette.toml"), manifest).unwrap();
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for (name, body) in sources {
        let path = src.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
}

fn read_manifest(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("lisette.toml")).unwrap()
}

#[test]
fn sync_promotes_transitive_still_imported() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux"] }
"#;
    let source = r#"import "go:github.com/gorilla/context"

fn main() {}
"#;
    write_project(tmp.path(), manifest, &[("main.lis", source)]);

    let output = run_sync(tmp.path());
    assert!(
        output.status.success(),
        "lis sync failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let after = read_manifest(tmp.path());
    assert!(
        after.contains(r#""github.com/gorilla/context" = "v1.1.1""#),
        "expected context promoted to a bare version string, got:\n{}",
        after
    );
    assert!(
        !after.contains("via"),
        "expected via field gone, got:\n{}",
        after
    );
}

#[test]
fn sync_removes_transitive_no_longer_imported() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux"] }
"#;
    write_project(tmp.path(), manifest, &[("main.lis", "fn main() {}\n")]);

    let output = run_sync(tmp.path());
    assert!(output.status.success());

    let after = read_manifest(tmp.path());
    assert!(
        !after.contains("gorilla/context"),
        "expected context removed, got:\n{}",
        after
    );
}

#[test]
fn sync_keeps_transitive_with_remaining_parents() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/mux" = "v1.8.0"
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux", "github.com/old/dead"] }
"#;
    write_project(tmp.path(), manifest, &[("main.lis", "fn main() {}\n")]);

    let output = run_sync(tmp.path());
    assert!(output.status.success());

    let after = read_manifest(tmp.path());
    assert!(
        after.contains("gorilla/context"),
        "expected context kept, got:\n{}",
        after
    );
    assert!(
        after.contains("gorilla/mux") && !after.contains("old/dead"),
        "expected via shortened to drop dead parent only, got:\n{}",
        after
    );
}

#[test]
fn sync_promotes_subpackage_via_longest_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"k8s.io/api" = { version = "v0.30.0", via = ["k8s.io/client-go"] }
"#;
    let source = r#"import "go:k8s.io/api/core/v1"

fn main() {}
"#;
    write_project(tmp.path(), manifest, &[("main.lis", source)]);

    let output = run_sync(tmp.path());
    assert!(output.status.success());

    let after = read_manifest(tmp.path());
    assert!(
        after.contains(r#""k8s.io/api" = "v0.30.0""#),
        "expected k8s.io/api promoted via longest-prefix subpackage match, got:\n{}",
        after
    );
}

#[test]
fn sync_no_op_on_clean_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/mux" = "v1.8.0"
"#;
    let source = r#"import "go:github.com/gorilla/mux"

fn main() {}
"#;
    write_project(tmp.path(), manifest, &[("main.lis", source)]);

    let before = read_manifest(tmp.path());
    let output = run_sync(tmp.path());
    assert!(output.status.success());

    let after = read_manifest(tmp.path());
    assert_eq!(
        before, after,
        "manifest must be byte-identical on no-op sync"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Manifest already in sync"),
        "expected already-in-sync message, got stderr:\n{}",
        stderr
    );
}

#[test]
fn sync_aborts_on_source_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux"] }
"#;
    let bad_source = "fn broken( {\n";
    write_project(tmp.path(), manifest, &[("main.lis", bad_source)]);

    let before = read_manifest(tmp.path());
    let output = run_sync(tmp.path());
    assert!(
        !output.status.success(),
        "lis sync must fail on parse error"
    );

    let after = read_manifest(tmp.path());
    assert_eq!(
        before, after,
        "manifest must be unchanged when sync aborts on parse error"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("parse"),
        "expected parse-error message, got stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("main.lis"),
        "error must point at the failing file, got stderr:\n{}",
        stderr
    );
}

#[test]
fn sync_aborts_with_clear_message_on_subpackage_dep() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/mux" = "v1.8.0"
"github.com/gorilla/mux/middleware" = "v1.8.0"
"#;
    write_project(tmp.path(), manifest, &[("main.lis", "fn main() {}\n")]);

    let output = run_sync(tmp.path());
    assert!(
        !output.status.success(),
        "lis sync must fail when a subpackage is declared as a dep"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("`github.com/gorilla/mux/middleware`")
            && stderr.contains("subpackage of `github.com/gorilla/mux`"),
        "expected targeted subpackage diagnostic, got stderr:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("invalid Go version"),
        "subpackage error must not be misframed as a version problem, got stderr:\n{}",
        stderr
    );
}

#[test]
fn sync_accepts_multi_module_monorepo_siblings() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"go.opentelemetry.io/otel" = { version = "v1.37.0", via = ["go.opentelemetry.io/contrib"] }
"go.opentelemetry.io/otel/sdk" = { version = "v1.37.0", via = ["go.opentelemetry.io/contrib"] }
"go.opentelemetry.io/otel/sdk/metric" = { version = "v1.36.0", via = ["go.opentelemetry.io/otel/sdk"] }
"#;
    write_project(tmp.path(), manifest, &[("main.lis", "fn main() {}\n")]);

    let output = run_sync(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("subpackage of"),
        "multi-module monorepo siblings must not be flagged as subpackages, got stderr:\n{}",
        stderr
    );
}
