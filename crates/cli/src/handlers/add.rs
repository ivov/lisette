use std::path::{Path, PathBuf};

use super::project::MutationProject;
use super::reconciliation::{
    ReplacedRoot, ReplacedRootMode, ResolvedDependency, RootWrite, apply_graph_to_document,
    declared_local_dirs, declared_replacements, open_manifest_document, reconcile_root,
    save_manifest_document,
};
use crate::go_cli;
use crate::output::{print_add_success, print_preview_notice, print_progress, print_warning};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::GoModule;
use stdlib::Target;

/// CLI-input dependency: the path the user typed, which may be a subpackage.
pub(super) struct ParsedDependency {
    pub(super) requested_package: String,
    pub(super) version: String,
}

struct AddPlan {
    project: MutationProject,
    dependency: ResolvedDependency,
    resolved_version: String,
    source: DependencySource,
}

enum DependencySource {
    Remote,
    Replacement(deps::ReplacementSource),
}

pub fn add(
    dep_string: Option<&str>,
    replace: Option<&str>,
    path: Option<&str>,
    script: Option<&str>,
) -> i32 {
    if let Some(script) = script {
        let Some(dep_string) = dep_string else {
            cli_error!(
                "Invalid dependency",
                "no dependency given",
                "Example: `lis add --script tool.lis google/uuid`"
            );
            return 1;
        };
        return super::script_add::run(std::path::Path::new(script), dep_string);
    }
    if let Some(path) = path {
        return add_local(path);
    }
    let Some(dep_string) = dep_string else {
        cli_error!(
            "Invalid dependency",
            "no dependency given",
            "Example: `lis add google/uuid@v1.6.0`"
        );
        return 1;
    };

    if let Some(replace) = replace {
        return add_replace(dep_string, replace);
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

    let plan = match setup_project(parsed_dep) {
        Ok(plan) => plan,
        Err(code) => return code,
    };

    run_add_pipeline(plan)
}

/// Expand a bare `owner/repo` to `github.com/owner/repo`; keep a non-GitHub path as-is.
fn normalize_module_path(path: &str) -> Result<String, String> {
    if deps::is_third_party(path) {
        Ok(path.to_string())
    } else if path.contains('/') {
        Ok(format!("github.com/{}", path))
    } else {
        Err(format!(
            "`{}` is not a valid module path; expected something like `github.com/owner/repo`",
            path
        ))
    }
}

/// `lis add <original> --replace <module>[@<version>]`: source `<original>` from another module.
fn add_replace(original_arg: &str, replace_spec: &str) -> i32 {
    let original = original_arg.trim();
    if original.contains('@') {
        cli_error!(
            "Invalid dependency",
            "with `--replace`, the dependency is the original module path, given without `@version`",
            "Example: `lis add github.com/df-mc/dragonfly --replace github.com/you/dragonfly@v1.2.0`"
        );
        return 1;
    }
    let original = match normalize_module_path(original) {
        Ok(p) => p,
        Err(msg) => {
            cli_error!(
                "Invalid dependency",
                msg,
                "Example: `lis add github.com/df-mc/dragonfly --replace github.com/you/dragonfly@v1.2.0`"
            );
            return 1;
        }
    };

    let (replace_raw, replace_query) = match replace_spec.rsplit_once('@') {
        Some((path, version)) if !path.is_empty() && !version.is_empty() => {
            (path.to_string(), version.to_string())
        }
        Some(_) => {
            cli_error!(
                "Invalid `--replace`",
                format!(
                    "`{}` is not of the form `<module>[@<version>]`",
                    replace_spec
                ),
                "Example: `--replace github.com/you/dragonfly@v1.2.0`"
            );
            return 1;
        }
        None => (replace_spec.to_string(), "latest".to_string()),
    };
    let replacement_path = match normalize_module_path(&replace_raw) {
        Ok(p) => p,
        Err(msg) => {
            cli_error!(
                "Invalid `--replace`",
                msg,
                "Example: `--replace github.com/you/dragonfly@v1.2.0`"
            );
            return 1;
        }
    };

    let plan = match setup_project_replace(&original, &replacement_path, &replace_query) {
        Ok(plan) => plan,
        Err(code) => return code,
    };

    run_add_pipeline(plan)
}

/// `lis add --path <dir>`: declare a local Go module, identified by its own `go.mod`.
fn add_local(path_arg: &str) -> i32 {
    let plan = match setup_project_local(path_arg) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    run_add_pipeline(plan)
}

fn setup_project_local(path_arg: &str) -> Result<AddPlan, i32> {
    let setup = MutationProject::open()?;
    print_preview_notice("Local Go modules", true);

    let invocation_dir = std::env::current_dir().map_err(|error| {
        error!(
            "failed to resolve directory",
            format!("could not read the current directory: {}", error)
        );
        1
    })?;
    let requested = invocation_dir.join(path_arg);
    let module_dir = requested.canonicalize().map_err(|_| {
        cli_error!(
            "Local module not found",
            format!("`{}` is not a directory", path_arg),
            "Pass the root directory of a Go module, e.g. `lis add --path ../auth`"
        );
        1
    })?;
    if !module_dir.is_dir() {
        cli_error!(
            "Local module not found",
            format!("`{}` is not a directory", path_arg),
            "Pass the root directory of a Go module, e.g. `lis add --path ../auth`"
        );
        return Err(1);
    }
    if !module_dir.join("go.mod").exists() {
        cli_error!(
            "Local module not found",
            format!("`{}` has no `go.mod`", path_arg),
            "Pass the root directory of a Go module, e.g. `lis add --path ../auth`"
        );
        return Err(1);
    }

    if !write_target_go_mod(&setup, None) {
        return Err(1);
    }
    let workspace = GoWorkspace::new(&setup.target_dir, &setup.typedef_cache_dir, Target::host());

    let summary = match workspace.read_go_mod_summary(&module_dir.join("go.mod")) {
        Ok(summary) => summary,
        Err(msg) => {
            error!("failed to read local module", msg);
            return Err(1);
        }
    };
    let module = summary.module_path;

    if !deps::is_third_party(&module) {
        cli_error!(
            "Local module not usable",
            format!(
                "`{}` declares module `{}`, which has no dot in its first path segment",
                path_arg, module
            ),
            "Give the module a dotted path in its `go.mod`, e.g. `example.com/auth`"
        );
        return Err(1);
    }

    let manifest_path = manifest_relative_path(&setup.root, &module_dir);
    let local_dep = deps::GoDependency::Replaced {
        source: deps::ReplacementSource::Local {
            path: manifest_path.clone(),
        },
        via: None,
    };

    if let Err(message) = deps::upsert_go_dependency(&setup.root, &module, &local_dep) {
        error!("failed to update manifest", message);
        return Err(1);
    }
    if !write_target_go_mod(&setup, Some((&module, local_dep))) {
        return Err(1);
    }

    print_progress(&format!("Adding local module {}", module));

    let resolved_version = deps::placeholder_require_version(&module);
    Ok(AddPlan {
        project: setup,
        dependency: ResolvedDependency {
            requested_package: module.clone(),
            canonical_module: module,
        },
        resolved_version,
        source: DependencySource::Replacement(deps::ReplacementSource::Local {
            path: manifest_path,
        }),
    })
}

fn manifest_relative_path(project_root: &Path, module_dir: &Path) -> String {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let root_components: Vec<_> = root.components().collect();
    let module_components: Vec<_> = module_dir.components().collect();

    let shared = root_components
        .iter()
        .zip(module_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    if shared == 0 {
        return module_dir.display().to_string();
    }

    let mut relative = PathBuf::new();
    for _ in shared..root_components.len() {
        relative.push("..");
    }
    for component in &module_components[shared..] {
        relative.push(component);
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    relative.display().to_string()
}

fn run_add_pipeline(plan: AddPlan) -> i32 {
    let project = &plan.project;
    let resolved_dep = &plan.dependency;
    let replacement = match &plan.source {
        DependencySource::Remote => None,
        DependencySource::Replacement(replacement) => Some(replacement),
    };
    let workspace = GoWorkspace::new(
        &project.target_dir,
        &project.typedef_cache_dir,
        Target::host(),
    );

    let mut replacements = declared_replacements(&project.manifest);
    match replacement {
        Some(replacement) => {
            replacements.insert(resolved_dep.canonical_module.clone(), replacement.clone());
        }
        None => {
            replacements.remove(&resolved_dep.canonical_module);
        }
    }

    let locals = declared_local_dirs(&replacements, &project.root);
    let module_graph = match reconcile_root(resolved_dep, &workspace, &replacements, &locals) {
        Ok(graph) => graph,
        Err(code) => return code,
    };

    let root_write = match replacement {
        Some(identity) => RootWrite::Replaced(ReplacedRoot {
            identity,
            mode: ReplacedRootMode::AddDirect,
        }),
        None => RootWrite::Remote {
            fallback_version: &plan.resolved_version,
        },
    };
    let mut manifest = match open_manifest_document(&project.root) {
        Ok(manifest) => manifest,
        Err(code) => return code,
    };
    let upgraded = match apply_graph_to_document(
        &resolved_dep.canonical_module,
        &mut manifest.document,
        &workspace,
        &module_graph,
        root_write,
    ) {
        Ok(u) => u,
        Err(code) => return code,
    };
    if let Err(message) = deps::finalize_via(&mut manifest.document, &[]) {
        error!("failed to update manifest", message);
        return 1;
    }
    if let Err(code) = save_manifest_document(&manifest) {
        return code;
    }

    let dep_version = module_graph
        .version(&resolved_dep.canonical_module)
        .map(str::to_string)
        .unwrap_or(plan.resolved_version);

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

    let replacement_label = replacement.map(|source| match source {
        deps::ReplacementSource::Module { path, version } => {
            crate::output::ReplacementLabel::Module { path, version }
        }
        deps::ReplacementSource::Local { path } => crate::output::ReplacementLabel::Local { path },
    });

    let edges = module_graph.edges();
    let versions = module_graph.versions();
    print_add_success(
        &resolved_dep.canonical_module,
        &dep_version,
        &edges,
        &versions,
        &upgraded_tuples,
        replacement_label,
    );

    0
}

const PRELUDE_GO_MODULE: &str = "github.com/ivov/lisette/prelude";

pub(super) fn parse_dep_string(input: &str) -> Result<ParsedDependency, String> {
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

    let requested_package = normalize_module_path(path)?;

    if requested_package == PRELUDE_GO_MODULE {
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

/// Write `target/go.mod` from the manifest, plus one `extra` dep not yet in it.
fn write_target_go_mod(setup: &MutationProject, extra: Option<(&str, deps::GoDependency)>) -> bool {
    let mut go_deps = setup.manifest.go_deps();
    if let Some((module, dep)) = extra {
        go_deps.insert(module.to_string(), dep);
    }
    write_target_go_mod_from(setup, go_deps)
}

/// Write `target/go.mod` from an explicit dependency set.
fn write_target_go_mod_from(
    setup: &MutationProject,
    go_deps: std::collections::BTreeMap<String, deps::GoDependency>,
) -> bool {
    let locator = deps::TypedefLocator::new(go_deps, Some(setup.root.clone()), Target::host());
    if let Err(msg) =
        go_cli::write_go_mod(&setup.target_dir, &setup.manifest.project.name, &locator)
    {
        error!("failed to write target/go.mod", msg);
        return false;
    }
    true
}

/// Remove any declared replacement for the module that owns `package`.
fn strip_replacement_for(
    go_deps: &mut std::collections::BTreeMap<String, deps::GoDependency>,
    package: &str,
) {
    let owner = go_deps
        .keys()
        .filter(|module| package == module.as_str() || package.starts_with(&format!("{module}/")))
        .max_by_key(|module| module.len())
        .cloned();
    if let Some(module) = owner
        && matches!(
            go_deps.get(&module),
            Some(deps::GoDependency::Replaced { .. })
        )
    {
        go_deps.remove(&module);
    }
}

fn setup_project(parsed_dep: ParsedDependency) -> Result<AddPlan, i32> {
    let setup = MutationProject::open()?;
    print_preview_notice("Third-party Go dependencies", true);
    let mut go_deps = setup.manifest.go_deps();
    strip_replacement_for(&mut go_deps, &parsed_dep.requested_package);
    if !write_target_go_mod_from(&setup, go_deps) {
        return Err(1);
    }

    let workspace = GoWorkspace::new(&setup.target_dir, &setup.typedef_cache_dir, Target::host());

    print_progress(&format!(
        "Fetching {}@{}",
        parsed_dep.requested_package, parsed_dep.version
    ));

    // `go get` accepts subpackage paths; `go list -m -json X@latest` does not.
    if let Err(msg) = workspace.go_get(GoModule {
        path: &parsed_dep.requested_package,
        version: &parsed_dep.version,
        replacement: None,
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

    Ok(AddPlan {
        project: setup,
        dependency: resolved,
        resolved_version: info.version,
        source: DependencySource::Remote,
    })
}

fn setup_project_replace(
    original: &str,
    replacement_path: &str,
    replace_query: &str,
) -> Result<AddPlan, i32> {
    let setup = MutationProject::open()?;
    print_preview_notice("Third-party Go dependencies", true);

    if !write_target_go_mod(&setup, None) {
        return Err(1);
    }

    let workspace = GoWorkspace::new(&setup.target_dir, &setup.typedef_cache_dir, Target::host());

    print_progress(&format!(
        "Resolving replacement {}@{}",
        replacement_path, replace_query
    ));
    let resolution = match workspace.resolve_replace(replacement_path, replace_query) {
        Ok(r) => r,
        Err(msg) => {
            error!("failed to resolve replacement", msg);
            return Err(1);
        }
    };

    if let Err(msg) =
        deps::check_version_matches_path(replacement_path, &resolution.resolved_version)
    {
        cli_error!(
            "Replacement is not usable",
            msg,
            "Publish the replacement at a path whose major version matches, e.g. `<module>/vN`"
        );
        return Err(1);
    }

    if resolution.declared_module != original {
        print_warning(&format!(
            "replacement `{}@{}` declares module `{}`, not `{}`. It builds only if its own imports use `{}`",
            replacement_path,
            resolution.resolved_version,
            resolution.declared_module,
            original,
            original
        ));
    }

    let replaced_dep = deps::GoDependency::Replaced {
        source: deps::ReplacementSource::Module {
            path: replacement_path.to_string(),
            version: resolution.resolved_version.clone(),
        },
        via: None,
    };
    if !write_target_go_mod(&setup, Some((original, replaced_dep))) {
        return Err(1);
    }

    let resolved = ResolvedDependency {
        requested_package: original.to_string(),
        canonical_module: original.to_string(),
    };

    Ok(AddPlan {
        project: setup,
        dependency: resolved,
        resolved_version: resolution.resolved_version.clone(),
        source: DependencySource::Replacement(deps::ReplacementSource::Module {
            path: replacement_path.to_string(),
            version: resolution.resolved_version,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_module_path_applies_github_shorthand() {
        assert_eq!(
            normalize_module_path("github.com/df-mc/dragonfly").unwrap(),
            "github.com/df-mc/dragonfly"
        );
        assert_eq!(
            normalize_module_path("df-mc/dragonfly").unwrap(),
            "github.com/df-mc/dragonfly"
        );
        assert_eq!(
            normalize_module_path("go.uber.org/zap").unwrap(),
            "go.uber.org/zap"
        );
        assert!(normalize_module_path("dragonfly").is_err());
    }

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

    #[test]
    fn strip_replacement_leaves_parent_when_child_remote_owns_package() {
        let mut go_deps = std::collections::BTreeMap::new();
        go_deps.insert(
            "go.opentelemetry.io/otel".to_string(),
            deps::GoDependency::Replaced {
                source: deps::ReplacementSource::Module {
                    path: "github.com/fork/otel".to_string(),
                    version: "v1.0.0".to_string(),
                },
                via: None,
            },
        );
        go_deps.insert(
            "go.opentelemetry.io/otel/sdk".to_string(),
            deps::GoDependency::Remote {
                version: "v1.0.0".to_string(),
                via: None,
            },
        );
        strip_replacement_for(&mut go_deps, "go.opentelemetry.io/otel/sdk");
        assert!(go_deps.contains_key("go.opentelemetry.io/otel"));
        assert!(go_deps.contains_key("go.opentelemetry.io/otel/sdk"));
    }

    #[test]
    fn strip_replacement_removes_longest_prefix_replaced_owner() {
        let mut go_deps = std::collections::BTreeMap::new();
        go_deps.insert(
            "go.opentelemetry.io/otel".to_string(),
            deps::GoDependency::Remote {
                version: "v1.0.0".to_string(),
                via: None,
            },
        );
        go_deps.insert(
            "go.opentelemetry.io/otel/sdk".to_string(),
            deps::GoDependency::Replaced {
                source: deps::ReplacementSource::Module {
                    path: "github.com/fork/otel-sdk".to_string(),
                    version: "v1.0.0".to_string(),
                },
                via: None,
            },
        );
        strip_replacement_for(&mut go_deps, "go.opentelemetry.io/otel/sdk");
        assert!(go_deps.contains_key("go.opentelemetry.io/otel"));
        assert!(!go_deps.contains_key("go.opentelemetry.io/otel/sdk"));
    }
}
