use std::collections::{BTreeSet, HashSet};
use std::process::Command;

use serde::Deserialize;
use std::env;
use std::fs;
use stdlib::Target;
use toml_edit::de as toml_de;

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
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
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

#[derive(Deserialize)]
struct DistWorkspace {
    dist: Dist,
}

#[derive(Deserialize)]
struct Dist {
    targets: Vec<String>,
}

fn go_target(triple: &str) -> Option<Target> {
    let (architecture, platform) = triple.split_once('-')?;

    let goarch = match architecture {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };

    let goos = if platform.contains("linux") {
        "linux"
    } else if platform.contains("darwin") {
        "darwin"
    } else if platform.contains("windows") {
        "windows"
    } else {
        return None;
    };

    Some(Target::new(goos, goarch))
}

#[test]
fn released_platforms_resolve_go_stdlib_packages() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../dist-workspace.toml");
    let text = fs::read_to_string(path).expect("read dist-workspace.toml");
    let workspace: DistWorkspace = toml_de::from_str(&text).expect("parse dist-workspace.toml");

    assert!(
        !workspace.dist.targets.is_empty(),
        "dist-workspace.toml lists no release targets"
    );

    let mut failures = Vec::new();
    let mut targets = Vec::new();
    for triple in &workspace.dist.targets {
        match go_target(triple) {
            Some(target) => targets.push(target),
            None => failures.push(format!("{}: no GOOS/GOARCH mapping", triple)),
        }
    }

    let mut packages = BTreeSet::new();
    for &target in &targets {
        packages.extend(stdlib::get_go_stdlib_packages(target));
    }

    for &target in &targets {
        for &package in &packages {
            if stdlib::get_go_stdlib_package_targets(package).is_some() {
                continue;
            }
            if stdlib::get_go_stdlib_typedef(package, target).is_none() {
                failures.push(format!("{}: no typedef for `{}`", target, package));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "every platform in dist-workspace.toml needs a GOOS/GOARCH mapping in `go_target` and Go stdlib typedefs. For a missing typedef, add the target to `_supported-targets` in the justfile, then run `just generate-stdlib-typedefs $(just _stdlib-typedef-version)`:\n{}",
        failures.join("\n")
    );
}
