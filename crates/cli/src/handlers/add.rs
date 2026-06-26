use std::fs::File;
use std::path::{Path, PathBuf};

use super::reconciliation::{
    ResolvedDependency, apply_graph_to_manifest, expand_unwalked_modules,
    rebuild_drifted_cache_entries, reconcile_module_graph, walk_typedef_cache,
};
use crate::go_cli;
use crate::lock::{acquire_mutation_lock, acquire_target_lock};
use crate::output::{print_add_success, print_preview_notice, print_progress};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::GoModule;
use stdlib::Target;

/// CLI-input dependency: the path the user typed, which may be a subpackage.
struct ParsedDependency {
    requested_package: String,
    version: String,
}

struct ProjectContext {
    project_root: PathBuf,
    target_dir: PathBuf,
    manifest: deps::Manifest,
    typedef_cache_dir: PathBuf,
    resolved_version: String,
    _mutation_lock: File,
    _target_lock: File,
}

pub fn add(dep_string: &str) -> i32 {
    if let Err(code) = go_cli::require_go() {
        return code;
    }

    let parsed_dep = match parse_dep_string(dep_string) {
        Ok(dep) => dep,
        Err(msg) => {
            cli_error!(
                "Invalid dependency",
                msg,
                "Example: `lis add google/uuid@v1.6.0`"
            );
            return 1;
        }
    };

    let (project_ctx, resolved_dep) = match setup_project(parsed_dep) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let workspace = GoWorkspace::new(
        &project_ctx.target_dir,
        &project_ctx.typedef_cache_dir,
        Target::host(),
    );

    let mut module_graph = match reconcile_module_graph(&resolved_dep, &workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let bindgenned = match walk_typedef_cache(&resolved_dep, &workspace, &mut module_graph) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if let Err(code) = expand_unwalked_modules(&workspace, &mut module_graph) {
        return code;
    }

    // Expansion above may MVS-upgrade modules whose typedefs the cache walk
    // already wrote at the old version, so refresh them at the new pin.
    rebuild_drifted_cache_entries(&workspace, &module_graph, &bindgenned);

    let upgraded = match apply_graph_to_manifest(
        &resolved_dep.canonical_module,
        &project_ctx.project_root,
        &project_ctx.manifest,
        &project_ctx.resolved_version,
        &workspace,
        &module_graph,
    ) {
        Ok(u) => u,
        Err(code) => return code,
    };

    let dep_version = module_graph
        .versions
        .get(&resolved_dep.canonical_module)
        .cloned()
        .unwrap_or(project_ctx.resolved_version);

    let upgraded_tuples: Vec<(&str, &str, &str)> = upgraded
        .iter()
        .map(|u| {
            (
                u.path.as_str(),
                u.old_version.as_str(),
                u.new_version.as_str(),
            )
        })
        .collect();

    print_add_success(
        &resolved_dep.canonical_module,
        &dep_version,
        &module_graph.edges,
        &module_graph.versions,
        &upgraded_tuples,
    );

    0
}

const PRELUDE_MODULE: &str = "github.com/ivov/lisette/prelude";

fn parse_dep_string(input: &str) -> Result<ParsedDependency, String> {
    let input = input.trim();
    if input.starts_with('-') {
        return Err(format!(
            "`{}` looks like a flag, but `lis add` does not accept flags",
            input
        ));
    }

    if let Some(hint) = detect_non_module_shape(input) {
        return Err(format!("`{}` {}", input, hint));
    }

    if input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("dependency string contains whitespace or control characters".to_string());
    }

    let (raw_path, version) = match input.rsplit_once('@') {
        Some((p, v)) if !p.is_empty() && !v.is_empty() => (p, v.to_string()),
        None if !input.is_empty() => (input, "latest".to_string()),
        _ => return Err(format!("Cannot parse `{}`", input)),
    };

    if raw_path.contains('@') {
        return Err(format!(
            "`{}` contains more than one `@` but expected `<module>@<version>`",
            input
        ));
    }

    let path = raw_path.trim_end_matches('/');
    if path.is_empty() {
        return Err(format!("Cannot parse `{}`", input));
    }

    if path.starts_with("./") || path.split('/').any(|s| s == "..") {
        return Err(format!(
            "`{}` is a relative path; `lis add` accepts only absolute Go module paths",
            path
        ));
    }
    if path.split('/').any(|s| s.is_empty()) {
        return Err(format!(
            "`{}` contains an empty segment (consecutive `/`); fix the path and retry",
            path
        ));
    }
    if !path.contains('/') && path.contains('.') {
        return Err(format!(
            "`{}` looks like a host without an owner/repo; module paths must include all path segments (e.g. `{}/owner/repo`)",
            path, path
        ));
    }

    if path.contains('%') {
        return Err(format!(
            "`{}` looks URL-encoded; use literal `/` instead of `%2F` in module paths",
            path
        ));
    }
    if let Some((module, sep, version)) = wrong_version_separator(path) {
        return Err(format!(
            "`{}{}{}` uses `{}` as a version separator; `lis add` uses `@`, like Go modules (try `{}@{}`)",
            module, sep, version, sep, module, version
        ));
    }
    if let Some(corrected) = miscased_known_host(path) {
        return Err(format!(
            "`{}` — Go module paths are case-sensitive (try `{}` instead)",
            path, corrected
        ));
    }
    if let Some(bad) = path
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~' | '/')))
    {
        return Err(format!(
            "`{}` contains `{}`, which is not allowed in a Go module path (only ASCII letters, digits, and `.-_~/`)",
            path, bad
        ));
    }

    let host = Target::host();
    if stdlib::get_go_stdlib_typedef(path, host).is_some() {
        return Err(format!(
            "`{}` is a Go standard library package; stdlib packages do not need `lis add` (just `import \"go:{}\"`)",
            path, path
        ));
    }
    if let Some(targets) = stdlib::get_go_stdlib_package_targets(path) {
        return Err(format!(
            "`{}` is a Go standard library package, but it is not available on `{}`. Available on: {}",
            path,
            host,
            stdlib::format_targets(targets),
        ));
    }

    let requested_package = if deps::is_third_party(path) {
        path.to_string()
    } else if path.contains('/') {
        format!("github.com/{}", path)
    } else {
        return Err(format!(
            "`{}` is not a valid module path; expected something like `github.com/owner/repo`",
            path
        ));
    };

    if requested_package == PRELUDE_MODULE {
        return Err(
            "the Lisette prelude is built into every project and cannot be added as a dependency"
                .to_string(),
        );
    }

    let version = if version == "latest" {
        version
    } else if version.starts_with('v') || version.starts_with('V') {
        format!("v{}", &version[1..])
    } else if looks_like_bare_semver(&version) {
        format!("v{}", version)
    } else {
        // Pass through branch names, commit hashes, HEAD, etc. unchanged so
        // `go get` can resolve them to pseudo-versions.
        version
    };

    Ok(ParsedDependency {
        requested_package,
        version,
    })
}

/// Detect the common typo where the user uses a non-`@` version separator
/// (Cargo's `^`, npm's `:`, a URL fragment `#`, or a `key=value` style `=`).
/// Returns `(module, sep, version)` when the suffix after the separator looks
/// version-shaped (`v1.2.3`, `1.2.3`, `v1`, etc.) so the caller can suggest
/// the right form.
fn wrong_version_separator(path: &str) -> Option<(&str, char, &str)> {
    for sep in ['#', '^', ':', '='] {
        if let Some((module, version)) = path.rsplit_once(sep)
            && !module.is_empty()
            && looks_like_version(version)
        {
            return Some((module, sep, version));
        }
    }
    None
}

fn looks_like_version(s: &str) -> bool {
    if s.contains('/') {
        return false;
    }
    let stripped = s.strip_prefix('v').unwrap_or(s);
    !stripped.is_empty() && stripped.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn looks_like_bare_semver(s: &str) -> bool {
    let core = s.split(['-', '+']).next().unwrap_or("");
    if core.is_empty() {
        return false;
    }
    core.split('.')
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// Detect common shapes that look like a published module path but are not:
/// browser URLs, SSH clone strings, absolute or home-directory filesystem
/// paths, dot-only paths. Returns a hint suffix the caller can append to the
/// rejected input.
fn detect_non_module_shape(s: &str) -> Option<&'static str> {
    if s.starts_with("https://") || s.starts_with("http://") {
        return Some("looks like a URL; strip the `https://` prefix and use the bare module path");
    }
    if s.starts_with("git@") && s.contains(':') {
        return Some(
            "looks like an SSH clone URL; use the module path form (e.g. `github.com/owner/repo`) instead",
        );
    }
    if s.starts_with('/') {
        return Some(
            "is an absolute filesystem path; `lis add` accepts only published Go module paths",
        );
    }
    if s.starts_with("~/") {
        return Some("is a home-directory path; `lis add` accepts only published Go module paths");
    }
    if s != ".." && !s.is_empty() && s.chars().all(|c| c == '.') {
        return Some("is not a valid Go module path");
    }
    None
}

/// Detect a case-only typo of a popular Go-module host. Returns the path with
/// the host segment lowercased so the caller can suggest it.
fn miscased_known_host(path: &str) -> Option<String> {
    const KNOWN_HOSTS: &[&str] = &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];
    let (first, rest) = path.split_once('/')?;
    for host in KNOWN_HOSTS {
        if first != *host && first.eq_ignore_ascii_case(host) {
            return Some(format!("{}/{}", host, rest));
        }
    }
    None
}

fn find_first_parent_module(
    path: &str,
    max_hops: usize,
    mut is_module: impl FnMut(&str) -> bool,
) -> Option<String> {
    let mut p = path;
    for _ in 0..max_hops {
        let pos = p.rfind('/')?;
        p = &p[..pos];
        if is_module(p) {
            return Some(p.to_string());
        }
    }
    None
}

fn enrich_with_parent_hint(workspace: &GoWorkspace, path: &str, msg: String) -> String {
    if !msg.contains("not found") && !msg.contains("No matching versions") {
        return msg;
    }
    let parent = find_first_parent_module(path, 3, |p| workspace.query_latest_version(p).is_ok());
    let Some(parent) = parent else {
        return msg;
    };
    let leaf = path.strip_prefix(&format!("{parent}/")).unwrap_or("");
    format!(
        "{msg}\n · help: `{parent}` is the published module; `{leaf}` is one of its sub-packages - try `lis add {parent}`"
    )
}

pub(crate) fn find_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut current: &Path = &cwd;
    loop {
        if current.join("lisette.toml").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn setup_project(
    parsed_dep: ParsedDependency,
) -> Result<(ProjectContext, ResolvedDependency), i32> {
    let project_root = match find_project_root() {
        Some(root) => root,
        None => {
            cli_error!(
                "No project found",
                "No `lisette.toml` in current directory or in any parent",
                "Run `lis new <name>` to create a project"
            );
            return Err(1);
        }
    };

    let manifest = match deps::parse_manifest(&project_root) {
        Ok(m) => m,
        Err(msg) => {
            cli_error!("Failed to read manifest", msg, "Fix `lisette.toml`");
            return Err(1);
        }
    };

    if let Err(msg) = deps::check_toolchain_version(&manifest) {
        let trimmed = msg
            .strip_prefix("Toolchain mismatch: ")
            .unwrap_or(&msg)
            .to_string();
        error!("toolchain mismatch", trimmed);
        return Err(1);
    }

    if let Err(msg) = deps::check_no_subpackage_deps(&manifest) {
        cli_error!(
            "Invalid `lisette.toml`",
            msg,
            "Fix `lisette.toml` and retry"
        );
        return Err(1);
    }

    if let Err(msg) = deps::validate_project_name(&manifest.project.name) {
        cli_error!(
            "Invalid project name",
            msg,
            "Rename `project.name` in `lisette.toml`"
        );
        return Err(1);
    }

    print_preview_notice("Third-party Go dependencies", true);

    let project_target_dir = project_root.join("target");
    if project_target_dir.is_file() {
        cli_error!(
            "Failed to set up target directory",
            "`target/` exists but is a file, not a directory",
            "Remove or move `target/` and retry"
        );
        return Err(1);
    }
    if let Err(e) = std::fs::create_dir_all(&project_target_dir) {
        error!(
            "failed to set up target directory",
            format!("Failed to create target directory: {}", e)
        );
        return Err(1);
    }

    let mutation_lock = acquire_mutation_lock(&project_target_dir)?;
    let target_lock = acquire_target_lock(&project_target_dir)?;

    let locator = deps::TypedefLocator::new(
        manifest.go_deps(),
        Some(project_root.clone()),
        Target::host(),
    );

    if let Err(msg) = go_cli::write_go_mod(&project_target_dir, &manifest.project.name, &locator) {
        error!("failed to write target/go.mod", msg);
        return Err(1);
    }

    let typedef_cache_dir = deps::typedef_cache_dir(&project_root);

    let workspace = GoWorkspace::new(&project_target_dir, &typedef_cache_dir, Target::host());

    print_progress(&format!(
        "Fetching {}@{}",
        parsed_dep.requested_package, parsed_dep.version
    ));

    // `go get` accepts subpackage paths; `go list -m -json X@latest` does not.
    if let Err(msg) = workspace.go_get(GoModule {
        path: &parsed_dep.requested_package,
        version: &parsed_dep.version,
    }) {
        let enriched = enrich_with_parent_hint(&workspace, &parsed_dep.requested_package, msg);
        error!("failed to download dependency", enriched);
        return Err(1);
    }

    let info = match workspace.find_containing_module(&parsed_dep.requested_package) {
        Ok(info) if !info.path.is_empty() && !info.version.is_empty() => info,
        Ok(_) => {
            error!(
                "failed to resolve containing module",
                format!(
                    "could not resolve containing module for `{}`",
                    parsed_dep.requested_package
                )
            );
            return Err(1);
        }
        Err(msg) => {
            error!("failed to resolve containing module", msg);
            return Err(1);
        }
    };

    let resolved = ResolvedDependency {
        requested_package: parsed_dep.requested_package,
        canonical_module: info.path,
    };

    let ctx = ProjectContext {
        project_root,
        target_dir: project_target_dir,
        manifest,
        typedef_cache_dir,
        resolved_version: info.version,
        _mutation_lock: mutation_lock,
        _target_lock: target_lock,
    };

    Ok((ctx, resolved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_first_ancestor_that_resolves() {
        let known = ["golang.org/x/net"];
        let parent =
            find_first_parent_module("golang.org/x/net/context", 3, |p| known.contains(&p));
        assert_eq!(parent.as_deref(), Some("golang.org/x/net"));
    }

    #[test]
    fn returns_none_when_no_ancestor_resolves() {
        let parent = find_first_parent_module("example.com/no/such/thing", 3, |_| false);
        assert!(parent.is_none());
    }

    #[test]
    fn returns_none_for_single_segment_path() {
        let mut probed = false;
        let parent = find_first_parent_module("singleton", 3, |_| {
            probed = true;
            true
        });
        assert!(parent.is_none());
        assert!(!probed, "single-segment path should not trigger any probe");
    }

    #[test]
    fn stops_at_max_hops() {
        let mut probes = Vec::new();
        let _ = find_first_parent_module("a/b/c/d/e", 2, |p| {
            probes.push(p.to_string());
            false
        });
        assert_eq!(probes, vec!["a/b/c/d", "a/b/c"]);
    }

    #[test]
    fn picks_nearest_module_when_multiple_ancestors_resolve() {
        let known = ["foo", "foo/bar"];
        let parent = find_first_parent_module("foo/bar/baz/qux", 5, |p| known.contains(&p));
        assert_eq!(parent.as_deref(), Some("foo/bar"));
    }
}
