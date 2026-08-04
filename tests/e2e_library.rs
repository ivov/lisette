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

fn write_manifest(project: &Path, name: &str) {
    fs::write(
        project.join("lisette.toml"),
        format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
}

fn scaffold(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("proj");
    fs::create_dir_all(project.join("src")).unwrap();
    write_manifest(&project, name);
    (dir, project)
}

#[test]
fn library_with_root_source_builds_importable_package() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/geo");
    fs::write(
        project.join("src/geo.lis"),
        "pub struct Point {\n  pub x: int,\n  pub y: int,\n}\n\npub fn origin() -> Point {\n  Point { x: 0, y: 0 }\n}\n",
    )
    .unwrap();

    let check = lis(&project, &["check"]);
    assert!(check.status.success(), "check failed: {}", combined(&check));

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "build failed: {}", combined(&build));
    assert!(combined(&build).contains("Library `github.com/acme/geo`"));

    let go = fs::read_to_string(project.join("target/geo.go")).unwrap();
    assert!(go.starts_with("package geo\n"), "got: {go}");
    let go_mod = fs::read_to_string(project.join("target/go.mod")).unwrap();
    assert!(
        go_mod.contains("module github.com/acme/geo"),
        "got: {go_mod}"
    );
    // `target/` is the public module: no tool-owned dirs beside the emitted Go.
    assert!(!project.join("target/bin").exists());
    assert!(!project.join("target/cache").exists());
}

#[test]
fn library_with_only_subpackages_emits_no_root_go() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/shapelib");
    fs::create_dir_all(project.join("src/shapes")).unwrap();
    fs::write(
        project.join("src/shapes/shapes.lis"),
        "pub struct Circle {\n  pub radius: int,\n}\n\npub fn unit() -> Circle {\n  Circle { radius: 1 }\n}\n",
    )
    .unwrap();

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "build failed: {}", combined(&build));
    assert!(project.join("target/shapes/shapes.go").exists());
    let root_go: Vec<_> = fs::read_dir(project.join("target"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "go"))
        .collect();
    assert!(root_go.is_empty(), "unexpected root .go: {root_go:?}");
}

#[test]
fn external_go_program_imports_the_library() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (dir, project) = scaffold("github.com/acme/geo");
    fs::write(
        project.join("src/geo.lis"),
        "pub struct Point {\n  pub x: int,\n  pub y: int,\n}\n\npub fn origin() -> Point {\n  Point { x: 0, y: 0 }\n}\n",
    )
    .unwrap();
    assert!(lis(&project, &["build"]).status.success());

    let consumer = dir.path().join("consumer");
    fs::create_dir_all(&consumer).unwrap();
    fs::write(
        consumer.join("go.mod"),
        "module example.com/consumer\n\ngo 1.25\n\nrequire github.com/acme/geo v0.0.0\n\nreplace github.com/acme/geo => ../proj/target\n",
    )
    .unwrap();
    fs::write(
        consumer.join("main.go"),
        "package main\n\nimport (\n\t\"fmt\"\n\n\t\"github.com/acme/geo\"\n)\n\nfunc main() {\n\tp := geo.Origin()\n\tfmt.Println(p.X, p.Y)\n}\n",
    )
    .unwrap();

    let run = Command::new("go")
        .args(["run", "."])
        .current_dir(&consumer)
        .env("GOFLAGS", "-mod=mod")
        .output()
        .expect("go run");
    assert!(
        run.status.success(),
        "consumer go run failed: {}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "0 0");
}

#[test]
fn bin_to_lib_conversion_removes_stale_root_and_bin() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/conv");
    fs::write(
        project.join("src/main.lis"),
        "import \"go:fmt\"\nfn main() { fmt.Println(\"hi\") }\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("src/lib")).unwrap();
    fs::write(
        project.join("src/lib/lib.lis"),
        "pub fn answer() -> int { 42 }\n",
    )
    .unwrap();
    assert!(lis(&project, &["build"]).status.success());
    assert!(project.join("target/main.go").exists());
    // The executable is tool-private, never in the public package namespace.
    assert!(project.join("target/.lisette/bin/conv").is_file());
    assert!(!project.join("target/bin").exists());

    fs::remove_file(project.join("src/main.lis")).unwrap();
    let build = lis(&project, &["build"]);
    assert!(
        build.status.success(),
        "rebuild failed: {}",
        combined(&build)
    );
    assert!(
        !project.join("target/main.go").exists(),
        "stale main.go survived"
    );
}

#[test]
fn run_on_a_library_reports_nothing_to_run() {
    let (_dir, project) = scaffold("github.com/acme/lib");
    fs::write(project.join("src/lib.lis"), "pub fn f() -> int { 1 }\n").unwrap();

    let run = lis(&project, &["run"]);
    assert!(!run.status.success());
    let out = combined(&run);
    assert!(out.contains("Nothing to run"), "got: {out}");
    assert!(out.contains("is a library"), "got: {out}");
}

#[test]
fn library_rejects_replaced_dependency_on_build_but_not_check() {
    let (_dir, project) = scaffold("github.com/acme/repl");
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/acme/repl\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n\"github.com/df-mc/dragonfly\" = { replacement = \"github.com/fork/dragonfly@v1.2.0\" }\n",
    )
    .unwrap();
    fs::write(project.join("src/repl.lis"), "pub fn f() -> int { 1 }\n").unwrap();

    let check = lis(&project, &["check"]);
    assert!(
        check.status.success(),
        "check should allow replace: {}",
        combined(&check)
    );

    let build = lis(&project, &["build"]);
    assert!(!build.status.success());
    assert!(combined(&build).contains("Replaced dependency in a library"));
}

#[test]
fn library_rejects_go_ignored_package_directory() {
    for bad in ["_hidden", "testdata", "vendor"] {
        let (_dir, project) = scaffold("github.com/acme/gign");
        fs::create_dir_all(project.join("src").join(bad)).unwrap();
        fs::write(
            project.join("src").join(bad).join("x.lis"),
            "pub fn f() -> int { 1 }\n",
        )
        .unwrap();
        let build = lis(&project, &["build"]);
        assert!(!build.status.success(), "`{bad}` should be rejected");
        assert!(combined(&build).contains("Go-ignored package directory"));
    }
}

#[test]
fn library_rejects_dotted_package_directory() {
    for bad in ["v1.2", "api/v1.2", "v2."] {
        let (_dir, project) = scaffold("github.com/acme/dotted");
        fs::write(
            project.join("src/dotted.lis"),
            "pub fn top() -> int { 1 }\n",
        )
        .unwrap();
        fs::create_dir_all(project.join("src").join(bad)).unwrap();
        fs::write(
            project.join("src").join(bad).join("x.lis"),
            "pub struct VConf { pub a: int }\n",
        )
        .unwrap();
        let build = lis(&project, &["build"]);
        let output = combined(&build);
        assert!(!build.status.success(), "`{bad}` should be rejected");
        assert!(
            output.contains("Dotted package directory"),
            "`{bad}` should be rejected, got: {output}"
        );
        assert!(
            !output.contains("INTERNAL COMPILER ERROR"),
            "`{bad}` must not reach type registration, got: {output}"
        );
    }
}

#[test]
fn binary_check_rejects_dotted_package_directory() {
    let (_dir, project) = scaffold("github.com/acme/dottedbin");
    fs::write(project.join("src/main.lis"), "fn main() { () }\n").unwrap();
    fs::create_dir_all(project.join("src/v1.2")).unwrap();
    fs::write(
        project.join("src/v1.2/vconf.lis"),
        "pub enum Mode { On, Off }\n",
    )
    .unwrap();
    let check = lis(&project, &["check"]);
    let output = combined(&check);
    assert!(!check.status.success());
    assert!(
        output.contains("Dotted package directory") && !output.contains("INTERNAL COMPILER ERROR"),
        "got: {output}"
    );
}

#[test]
fn library_rejects_platform_suffixed_source() {
    for bad in [
        "api_windows.lis",
        "api_linux_amd64.lis",
        "api_amd64p32.lis",
        "api_sparc64.lis",
        "api_windows.extra.lis",
    ] {
        let (_dir, project) = scaffold("github.com/acme/plat");
        fs::write(project.join("src").join(bad), "pub fn f() -> int { 1 }\n").unwrap();
        let build = lis(&project, &["build"]);
        assert!(!build.status.success(), "`{bad}` is a Go build constraint");
        assert!(
            combined(&build).contains("Platform-suffixed source file"),
            "`{bad}` should be rejected, got: {}",
            combined(&build)
        );
    }
}

#[test]
fn library_accepts_names_that_only_look_platform_suffixed() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/ok");
    fs::write(project.join("src/windows.lis"), "pub fn a() -> int { 1 }\n").unwrap();
    fs::write(
        project.join("src/my_helper.lis"),
        "pub fn b() -> int { 2 }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/helper_utils.lis"),
        "pub fn c() -> int { 3 }\n",
    )
    .unwrap();
    let build = lis(&project, &["build"]);
    assert!(
        build.status.success(),
        "no build constraint here: {}",
        combined(&build)
    );
}

#[test]
fn library_rejects_go_ignored_test_file() {
    let (_dir, project) = scaffold("github.com/acme/gtest");
    fs::write(project.join("src/gtest.lis"), "pub fn f() -> int { 1 }\n").unwrap();
    fs::write(
        project.join("src/_smoke.test.lis"),
        "#[test]\nfn smoke() {}\n",
    )
    .unwrap();
    let build = lis(&project, &["build"]);
    assert!(
        !build.status.success(),
        "a skipped `_test.go` runs no tests"
    );
    assert!(combined(&build).contains("Go-ignored source file"));
}

#[test]
fn rejected_library_build_writes_no_go_output() {
    let (_dir, project) = scaffold("github.com/acme/nomut");
    fs::write(
        project.join("lisette.toml"),
        "[project]\nname = \"github.com/acme/nomut\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n\"github.com/df-mc/dragonfly\" = { replacement = \"github.com/fork/dragonfly@v1.2.0\" }\n",
    )
    .unwrap();
    fs::write(project.join("src/nomut.lis"), "pub fn f() -> int { 1 }\n").unwrap();

    let build = lis(&project, &["build"]);
    assert!(!build.status.success());
    assert!(
        !project.join("target/go.mod").exists(),
        "a rejected build must not write `target/go.mod`"
    );
    let emitted: Vec<_> = fs::read_dir(project.join("target"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "go"))
        .collect();
    assert!(emitted.is_empty(), "unexpected Go output: {emitted:?}");
}

#[test]
fn library_warns_when_the_module_path_is_not_fetchable() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("mylib");
    fs::write(project.join("src/mylib.lis"), "pub fn f() -> int { 1 }\n").unwrap();
    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "{}", combined(&build));
    assert!(
        combined(&build).contains("not fetchable"),
        "a dotless module path cannot be imported elsewhere: {}",
        combined(&build)
    );

    let (_dir2, fetchable) = scaffold("github.com/acme/mylib");
    fs::write(fetchable.join("src/mylib.lis"), "pub fn f() -> int { 1 }\n").unwrap();
    let build = lis(&fetchable, &["build"]);
    assert!(build.status.success(), "{}", combined(&build));
    assert!(!combined(&build).contains("not fetchable"));
}

#[test]
fn library_may_have_a_bin_package() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/binmod");
    fs::create_dir_all(project.join("src/bin/tools")).unwrap();
    fs::write(project.join("src/lib.lis"), "pub fn root() -> int { 1 }\n").unwrap();
    fs::write(
        project.join("src/bin/b.lis"),
        "pub fn helper() -> int { 2 }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/bin/tools/tool.lis"),
        "pub fn tool() -> int { 3 }\n",
    )
    .unwrap();

    for pass in ["cold", "cached"] {
        let build = lis(&project, &["build"]);
        assert!(
            build.status.success(),
            "{pass} build failed: {}",
            combined(&build)
        );
        for emitted in ["target/bin/b.go", "target/bin/tools/tool.go"] {
            assert!(
                project.join(emitted).exists(),
                "{pass} build removed `{emitted}`"
            );
        }
    }
}

#[test]
fn subpackage_only_library_with_bin_package_keeps_its_output() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/onlybin");
    fs::create_dir_all(project.join("src/bin/tools")).unwrap();
    fs::write(
        project.join("src/bin/tools/tool.lis"),
        "pub fn tool() -> int { 3 }\n",
    )
    .unwrap();

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "{}", combined(&build));
    assert!(project.join("target/bin/tools/tool.go").exists());
}

#[test]
fn binary_named_like_a_bin_subpackage_converts_to_a_library() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    // Building as a binary puts the executable under `target/.lisette/bin`, out
    // of the public package namespace, so a later `src/bin/tools` package cannot
    // collide with it.
    let (_dir, project) = scaffold("github.com/acme/tools");
    fs::write(
        project.join("src/main.lis"),
        "import \"go:fmt\"\nfn main() { fmt.Println(\"hi\") }\n",
    )
    .unwrap();
    assert!(lis(&project, &["build"]).status.success());
    assert!(project.join("target/.lisette/bin/tools").is_file());
    assert!(!project.join("target/bin").exists());

    fs::remove_file(project.join("src/main.lis")).unwrap();
    fs::create_dir_all(project.join("src/bin/tools")).unwrap();
    fs::write(project.join("src/lib.lis"), "pub fn root() -> int { 1 }\n").unwrap();
    fs::write(
        project.join("src/bin/tools/tool.lis"),
        "pub fn tool() -> int { 3 }\n",
    )
    .unwrap();

    let build = lis(&project, &["build"]);
    assert!(
        build.status.success(),
        "conversion failed: {}",
        combined(&build)
    );
    assert!(project.join("target/bin/tools/tool.go").is_file());
}

#[test]
fn subpackage_only_library_still_runs_root_tests() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/roottest");
    fs::create_dir_all(project.join("src/core")).unwrap();
    fs::write(
        project.join("src/core/core.lis"),
        "pub fn base() -> int { 21 }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/root.test.lis"),
        "import \"core\"\n\n#[test]\nfn root_test_sees_subpackage() {\n  assert core.base() == 21\n}\n",
    )
    .unwrap();

    let test = lis(&project, &["test"]);
    assert!(
        test.status.success(),
        "a subpackages-only library must still run root tests: {}",
        combined(&test)
    );
    assert!(combined(&test).contains("root_test_sees_subpackage"));

    let build = lis(&project, &["build"]);
    assert!(build.status.success(), "{}", combined(&build));
    let root_go: Vec<_> = fs::read_dir(project.join("target"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "go"))
        .collect();
    assert!(
        root_go.is_empty(),
        "a plain build still emits no root package: {root_go:?}"
    );
}

#[test]
fn binary_also_rejects_go_ignored_names() {
    let (_dir, project) = scaffold("github.com/acme/binguard");
    fs::create_dir_all(project.join("src/api")).unwrap();
    fs::write(
        project.join("src/main.lis"),
        "import \"go:fmt\"\nimport \"api\"\nfn main() { fmt.Println(api.always()) }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/api/api.lis"),
        "pub fn always() -> int { 1 }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/api/api_windows.lis"),
        "pub fn only_windows() -> int { 2 }\n",
    )
    .unwrap();

    let build = lis(&project, &["build"]);
    assert!(
        !build.status.success(),
        "Go drops the file from a binary just as silently"
    );
    assert!(combined(&build).contains("Platform-suffixed source file"));
}

#[test]
fn empty_src_reports_no_lisette_sources() {
    let (_dir, project) = scaffold("github.com/acme/empty");
    let build = lis(&project, &["build"]);
    assert!(!build.status.success());
    assert!(combined(&build).contains("No Lisette sources"));
}

#[test]
fn library_public_api_survives_dead_code_elimination() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("github.com/acme/api");
    fs::write(
        project.join("src/api.lis"),
        "pub fn exported_with_no_caller() -> int { 7 }\n",
    )
    .unwrap();
    assert!(lis(&project, &["build"]).status.success());
    let go = fs::read_to_string(project.join("target/api.go")).unwrap();
    assert!(
        go.contains("func ExportedWithNoCaller()"),
        "pub API was stripped: {go}"
    );
}

#[test]
fn major_version_suffix_uses_pre_version_package_name() {
    if !go_available() {
        eprintln!("skipping: `go` not found");
        return;
    }
    let (_dir, project) = scaffold("example.com/acme/lib/v2");
    fs::write(project.join("src/lib.lis"), "pub fn f() -> int { 1 }\n").unwrap();
    assert!(lis(&project, &["build"]).status.success());
    let go = fs::read_to_string(project.join("target/lib.go")).unwrap();
    assert!(go.starts_with("package lib\n"), "got: {go}");
}
