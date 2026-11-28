//! Type-checks generated Go with `go/types` (no compile, no link), and compares failures with a failing list.

use std::any::Any;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::go::repo_root;

/// The first line of a panic payload.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_default();
    message.lines().next().unwrap_or_default().to_string()
}

/// Type-checks each package of the Go module in `module_dir`, keyed by package directory.
pub fn type_check_module(module_dir: &Path) -> BTreeMap<String, Vec<String>> {
    let output = Command::new(type_checker_binary())
        .arg(module_dir)
        .current_dir(module_dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn go_type_checker");
    assert!(
        output.status.success(),
        "go_type_checker failed in {}:\n{}",
        module_dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((package, message)) = line.split_once('\t') else {
            continue;
        };
        errors
            .entry(package.to_string())
            .or_default()
            .push(message.to_string());
    }
    errors
}

fn type_checker_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| build_go_helper("_harness/go_type_checker", "go_type_checker"))
        .clone()
}

pub fn build_go_helper(source: &str, name: &str) -> PathBuf {
    let binary = repo_root().join(format!("target/{name}/{name}"));
    let output = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&binary)
        .arg(".")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(source))
        .output()
        .unwrap_or_else(|error| panic!("spawn go build for {name}: {error}"));
    assert!(
        output.status.success(),
        "go build of {name} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

/// Recreates `dir` as an empty directory.
pub fn fresh_dir(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap_or_else(|error| panic!("create {}: {error}", dir.display()));
}
