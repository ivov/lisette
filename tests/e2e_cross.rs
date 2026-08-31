use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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

fn lis_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(repo().join("Cargo.toml"))
        .args(["-p", "lisette", "--"])
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to invoke lisette")
}

fn lis(project: &Path, args: &[&str]) -> Output {
    let mut all: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    all.push(project.display().to_string());
    lis_in(&repo(), &all.iter().map(String::as_str).collect::<Vec<_>>())
}

fn windows_host() -> bool {
    stdlib::Target::host().goos == "windows"
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn scaffold(root: &Path, name: &str, main: &str) -> PathBuf {
    let project = root.join(name);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
    fs::write(project.join("src/main.lis"), main).unwrap();
    project
}

#[cfg(unix)]
fn scaffold_with_package(root: &Path) -> PathBuf {
    let project = scaffold(root, "cached", CACHED_MAIN);
    fs::create_dir_all(project.join("src/util")).unwrap();
    fs::write(project.join("src/util/util.lis"), CACHED_UTIL).unwrap();
    project
}

/// The file and the manifest agree with each other, and disagree with what this
/// target emits, which is what Go from another target looks like.
#[cfg(unix)]
fn plant_foreign_go(project: &Path) {
    let go = project.join("target/util/util.go");
    let manifest = project.join("target/.lisette/emit-manifest");
    let mut source = fs::read_to_string(&go).unwrap();
    source.push_str("\n// PLANTED\n");
    fs::write(&go, source).unwrap();

    let rewritten: String = fs::read_to_string(&manifest)
        .unwrap()
        .lines()
        .map(|line| match line.strip_prefix("util/util.go\t") {
            Some(rest) => {
                let tail = rest.split_once('\t').map(|(_, tail)| tail).unwrap_or("");
                format!("util/util.go\t0000000000000000\t{tail}\n")
            }
            None => format!("{line}\n"),
        })
        .collect();
    fs::write(&manifest, rewritten).unwrap();
}

#[cfg(unix)]
fn planted_go_survives(project: &Path) -> bool {
    fs::read_to_string(project.join("target/util/util.go"))
        .unwrap()
        .contains("PLANTED")
}

#[cfg(unix)]
const CACHED_MAIN: &str = r#"import "go:fmt"
import "util"

fn main() {
  fmt.Println(util.twice(21))
}
"#;

#[cfg(unix)]
const CACHED_UTIL: &str = r#"pub fn twice(n: int) -> int {
  n * 2
}
"#;

#[cfg(unix)]
const CACHED_UTIL_EDITED: &str = r#"pub fn twice(n: int) -> int {
  n + n
}
"#;

const PORTABLE_MAIN: &str = r#"import "go:fmt"
import "go:path/filepath"

fn main() {
  fmt.Println(filepath.Join("a", "b"))
}
"#;

const LINUX_ONLY_MAIN: &str = r#"import "go:fmt"
import "go:syscall"

fn main() {
  fmt.Println(syscall.EpollCreate1(0))
}
"#;

const SCRIPT_MAIN: &str = r#"import "go:fmt"

fn main() {
  fmt.Println("hi")
}
"#;

#[test]
fn a_cross_build_keeps_one_binary_per_target() {
    if !go_available() || windows_host() {
        eprintln!("skipping e2e_cross: `go` not found, or the host is windows");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold(tmp.path(), "portable", PORTABLE_MAIN);
    let bin = project.join("target/.lisette/bin");

    let host = lis(&project, &["build"]);
    assert!(host.status.success(), "{}", combined(&host));
    assert!(bin.join("portable").is_file(), "{}", combined(&host));

    for goarch in ["amd64", "arm64"] {
        let target = format!("windows/{goarch}");
        let cross = lis(&project, &["build", "--target", &target]);
        assert!(cross.status.success(), "{}", combined(&cross));

        let exe = bin.join(format!("windows_{goarch}")).join("portable.exe");
        let bytes = fs::read(&exe).unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
        assert_eq!(&bytes[..2], b"MZ", "{} is not a PE binary", exe.display());
    }

    assert!(bin.join("portable").is_file());
    assert!(bin.join("windows_amd64/portable.exe").is_file());
    assert!(bin.join("windows_arm64/portable.exe").is_file());
}

#[test]
fn a_cross_script_build_keeps_one_binary_per_target() {
    if !go_available() || windows_host() {
        eprintln!("skipping e2e_cross: `go` not found, or the host is windows");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    fs::write(dir.join("greet.lis"), SCRIPT_MAIN).unwrap();

    let host = lis_in(dir, &["build", "greet.lis"]);
    assert!(host.status.success(), "{}", combined(&host));
    assert!(dir.join("greet").is_file(), "{}", combined(&host));

    let cross = lis_in(dir, &["build", "--target", "windows/amd64", "greet.lis"]);
    assert!(cross.status.success(), "{}", combined(&cross));

    let exe = dir.join("greet_windows_amd64.exe");
    let bytes = fs::read(&exe).unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
    assert_eq!(&bytes[..2], b"MZ", "{} is not a PE binary", exe.display());
    assert!(dir.join("greet").is_file());

    let named = lis_in(
        dir,
        &["build", "--target", "linux/amd64", "greet.lis", "-o", "out"],
    );
    assert!(named.status.success(), "{}", combined(&named));
    assert!(dir.join("out").is_file(), "{}", combined(&named));
}

#[test]
fn a_host_target_named_explicitly_keeps_the_short_path() {
    if !go_available() {
        eprintln!("skipping e2e_cross: `go` not found");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold(tmp.path(), "portable", PORTABLE_MAIN);
    let host = stdlib::Target::host().to_string();

    let build = lis(&project, &["build", "--target", &host]);
    assert!(build.status.success(), "{}", combined(&build));

    let bin = project.join("target/.lisette/bin");
    assert!(bin.join("portable").is_file() || bin.join("portable.exe").is_file());
    assert!(!bin.join(stdlib::Target::host().cache_segment()).exists());
}

#[test]
fn a_check_resolves_the_named_platform_and_no_other() {
    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold(tmp.path(), "epoll", LINUX_ONLY_MAIN);

    let linux = lis(&project, &["check", "--target", "linux/amd64"]);
    assert!(linux.status.success(), "{}", combined(&linux));

    let darwin = lis(&project, &["check", "--target", "darwin/arm64"]);
    assert!(!darwin.status.success(), "{}", combined(&darwin));
    assert!(
        combined(&darwin).contains("EpollCreate1"),
        "{}",
        combined(&darwin)
    );

    let linux_again = lis(&project, &["check", "--target", "linux/amd64"]);
    assert!(linux_again.status.success(), "{}", combined(&linux_again));
}

/// A run that fails part way through must not leave the next one trusting Go
/// that two targets wrote between them.
#[cfg(unix)]
#[test]
fn a_failed_emit_leaves_no_target_marker() {
    if !go_available() {
        eprintln!("skipping e2e_cross: `go` not found");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold_with_package(tmp.path());
    let marker = project.join("target/.lisette/emit.target");

    let first = lis(&project, &["emit"]);
    assert!(first.status.success(), "{}", combined(&first));
    assert_eq!(
        fs::read_to_string(&marker).unwrap().trim(),
        stdlib::Target::host().to_string()
    );

    plant_foreign_go(&project);
    let warm = lis(&project, &["emit"]);
    assert!(warm.status.success(), "{}", combined(&warm));
    assert!(
        planted_go_survives(&project),
        "a warm emit must reuse the Go it already wrote"
    );

    let emitted_go = project.join("target/util/util.go");
    set_writable(&emitted_go, false);
    fs::write(project.join("src/util/util.lis"), CACHED_UTIL_EDITED).unwrap();
    let interrupted = lis(&project, &["emit"]);
    set_writable(&emitted_go, true);

    let report = combined(&interrupted);
    assert!(
        !interrupted.status.success(),
        "the fixture must make the Go write fail: {report}"
    );
    assert!(
        report.contains("Failed to write") && report.contains("util.go"),
        "the emit must fail on the Go write, not somewhere later: {report}"
    );
    assert!(
        !marker.exists(),
        "an emit that failed after touching the Go must leave no marker"
    );
}

/// If the marker cannot go, neither can the Go, or the old target would keep a
/// claim on output this run replaced.
#[cfg(unix)]
#[test]
fn an_unclearable_marker_stops_the_emit() {
    if !go_available() {
        eprintln!("skipping e2e_cross: `go` not found");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold_with_package(tmp.path());
    let first = lis(&project, &["emit"]);
    assert!(first.status.success(), "{}", combined(&first));

    let state_dir = project.join("target/.lisette");
    set_writable(&state_dir, false);
    fs::write(project.join("src/util/util.lis"), CACHED_UTIL_EDITED).unwrap();
    let blocked = lis(&project, &["emit"]);
    set_writable(&state_dir, true);

    assert!(
        !blocked.status.success(),
        "an emit that cannot clear the marker must not touch the Go: {}",
        combined(&blocked)
    );
    assert_eq!(
        fs::read_to_string(state_dir.join("emit.target"))
            .unwrap()
            .trim(),
        stdlib::Target::host().to_string()
    );
}

#[cfg(unix)]
fn set_writable(path: &Path, writable: bool) {
    let mut mode = fs::metadata(path).unwrap().permissions();
    let bits = mode.mode();
    mode.set_mode(if writable {
        bits | 0o200
    } else {
        bits & !0o222
    });
    fs::set_permissions(path, mode).unwrap();
}

#[test]
fn run_and_test_send_the_target_flag_to_build() {
    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold(tmp.path(), "portable", PORTABLE_MAIN);

    for command in ["run", "test"] {
        let output = lis(&project, &[command, "--target", "linux/amd64"]);
        let text = combined(&output);
        assert!(!output.status.success(), "{text}");
        assert!(text.contains("lis build --target"), "{text}");
    }
}

#[test]
fn an_unlisted_target_fails_at_the_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let project = scaffold(tmp.path(), "portable", PORTABLE_MAIN);

    for target in ["freebsd/amd64", "linux", "js/wasm"] {
        let output = lis(&project, &["check", "--target", target]);
        let text = combined(&output);
        assert!(!output.status.success(), "{text}");
        assert!(text.contains("`--target` accepts"), "{text}");
        assert!(!text.contains("unknown_go_stdlib_package"), "{text}");
    }
}
