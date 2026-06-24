use std::collections::HashSet;
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    dependencies: Vec<Dep>,
}

#[derive(Deserialize)]
struct Dep {
    name: String,
    req: String,
    kind: Option<String>,
}

/// Internal sibling crates must be exact-pinned so registry installs of an older
/// `lisette` cannot resolve newer sibling libraries and mix releases.
#[test]
fn internal_crate_deps_are_exact_pinned() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            manifest,
        ])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Metadata = serde_json::from_slice(&output.stdout).expect("parse cargo metadata");

    let members: HashSet<&str> = metadata.packages.iter().map(|p| p.name.as_str()).collect();

    let mut violations = Vec::new();
    for package in &metadata.packages {
        let expected = format!("={}", package.version);
        for dep in &package.dependencies {
            let is_sibling = members.contains(dep.name.as_str());
            let is_propagated = matches!(dep.kind.as_deref(), None | Some("build"));
            if is_sibling && is_propagated && dep.req != expected {
                violations.push(format!(
                    "{} -> {}: requirement `{}`, expected `{}`",
                    package.name, dep.name, dep.req, expected
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "internal crate dependencies must be exact-pinned to the workspace version:\n{}",
        violations.join("\n")
    );
}
