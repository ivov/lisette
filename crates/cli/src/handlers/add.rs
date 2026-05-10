use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::go_cli;
use crate::lock::{acquire_mutation_lock, acquire_target_lock};
use crate::output::{print_add_success, print_preview_notice, print_progress, print_warning};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::{GoModule, remove_go_dep, resolve_empty_via, trim_dead_via_parents, upsert_go_dep};
use stdlib::Target;

/// CLI-input dependency: the path the user typed, which may be a subpackage.
struct ParsedDependency {
    requested_package: String,
    version: String,
}

/// `ParsedDependency` after `setup_project` has resolved its containing module.
struct ResolvedDependency {
    requested_package: String,
    canonical_module: String,
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

struct GraphResult {
    /// Final MVS-selected version for each reconciled module, e.g.
    /// `{ "github.com/gorilla/mux" → "v1.8.1" }`.
    versions: HashMap<String, String>,
    /// For each reconciled module, the third-party modules it imports
    /// via its typedefs, e.g. `{ "mux" → ["context"] }`.
    edges: HashMap<String, Vec<String>>,
    /// Modules whose `find_third_party_modules` result is recorded in
    /// `edges`. Cache-walk inserts go in `versions` only; the post-walk
    /// expansion pass catches them up before manifest application.
    expanded: HashSet<String>,
}

impl GraphResult {
    /// Invert `edges` into a `module → parents` map, excluding the added root.
    fn transitive_map(&self, added_module: &str) -> HashMap<String, Vec<String>> {
        let mut transitives: HashMap<String, Vec<String>> = HashMap::new();
        for (parent, children) in &self.edges {
            for child in children {
                if child != added_module {
                    let parents = transitives.entry(child.clone()).or_default();
                    if !parents.contains(parent) {
                        parents.push(parent.clone());
                    }
                }
            }
        }
        for parents in transitives.values_mut() {
            parents.sort();
        }
        transitives
    }
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
        &project_ctx,
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

    print_preview_notice();

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

/// Manifest walk: BFS the third-party module subgraph from `dep.canonical_module`
/// via `go list -json M/...`. Module-grained so the manifest declares every
/// module a future subpackage import could reach; the outer loop converges
/// MVS drift since MVS only moves upward.
fn reconcile_module_graph(
    dep: &ResolvedDependency,
    workspace: &GoWorkspace,
) -> Result<GraphResult, i32> {
    let canonical_module = dep.canonical_module.as_str();

    let mut module_versions: HashMap<String, String> = HashMap::new();
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut expanded: HashSet<String> = HashSet::new();
    let mut failed_transitives: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = vec![canonical_module.to_string()];

    loop {
        while let Some(module_path) = queue.pop() {
            let is_explicit = module_path == canonical_module;

            let module_version = match workspace.query_version(&module_path) {
                Ok(v) => v,
                Err(msg) => {
                    if is_explicit {
                        error!("failed to resolve module version", msg);
                        return Err(1);
                    }
                    if failed_transitives.insert(module_path.clone()) {
                        print_warning(&format!("skipping transitive {}: {}", module_path, msg));
                    }
                    continue;
                }
            };

            if module_versions
                .get(&module_path)
                .is_some_and(|v| *v == module_version)
            {
                continue;
            }

            if !is_explicit && !module_versions.contains_key(&module_path) {
                print_progress(&format!("Resolving transitive dep {}", module_path));
            }

            let listed = match workspace.find_third_party_modules(&module_path) {
                Ok(l) => l,
                Err(msg) => {
                    if is_explicit {
                        error!("failed to scan transitive modules", msg);
                        return Err(1);
                    }
                    if failed_transitives.insert(module_path.clone()) {
                        print_warning(&format!("skipping transitive {}: {}", module_path, msg));
                    }
                    continue;
                }
            };

            if !listed.package_errors.is_empty() && is_explicit {
                let combined: String = listed
                    .package_errors
                    .iter()
                    .map(|e| format!("\n  · {}: {}", e.package, e.message))
                    .collect();
                error!(
                    "could not load all packages of dependency",
                    format!(
                        "`go list` reported errors in `{}`:{}",
                        module_path, combined
                    )
                );
                return Err(1);
            }
            for err in &listed.package_errors {
                print_warning(&format!(
                    "{}: package error in `{}`: {}",
                    module_path, err.package, err.message
                ));
            }

            module_versions.insert(module_path.clone(), module_version);
            edges.insert(module_path.clone(), listed.modules.clone());
            expanded.insert(module_path);

            for next in listed.modules {
                queue.push(next);
            }
        }

        let drift = detect_mvs_drift(workspace, &module_versions);
        if let Some((module, msg)) = drift.errors.first() {
            error!(
                "failed to resolve module version",
                format!("{}: {}", module, msg)
            );
            return Err(1);
        }
        if drift.upgraded.is_empty() {
            break;
        }
        for (module, _) in drift.upgraded {
            queue.push(module);
        }
    }

    if !failed_transitives.is_empty() {
        print_warning(&format!(
            "{} transitive dep(s) skipped; importing them later will fail until they are bindable",
            failed_transitives.len()
        ));
    }

    Ok(GraphResult {
        versions: module_versions,
        edges,
        expanded,
    })
}

/// Cache walk: bindgen the requested package, then recurse into each
/// typedef's own `go:` imports. Sibling subpackages stay cache misses for
/// the locator to handle on first access. Returns each bindgenned
/// `(module, version, package)` so any later MVS drift in
/// `expand_unwalked_modules` can re-reconcile at the new pin.
fn walk_typedef_cache(
    dep: &ResolvedDependency,
    workspace: &GoWorkspace,
    module_graph: &mut GraphResult,
) -> Result<Vec<BindgennedPackage>, i32> {
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut queue: Vec<(String, String, String)> = Vec::new();
    let mut bindgenned: Vec<BindgennedPackage> = Vec::new();

    let seed_packages = seed_cache_walk(
        &dep.canonical_module,
        &dep.requested_package,
        workspace,
        &mut queue,
    )?;

    while let Some((module_path, version, package_path)) = queue.pop() {
        if !visited.insert((module_path.clone(), package_path.clone())) {
            continue;
        }

        let is_seed = seed_packages.contains(&(module_path.clone(), package_path.clone()));
        let module = GoModule {
            path: &module_path,
            version: &version,
        };

        match workspace.reconcile_package(module, &package_path) {
            Ok(stubs) => {
                warn_stubbed(&stubs);
                bindgenned.push(BindgennedPackage {
                    module: module_path.clone(),
                    version: version.clone(),
                    package: package_path.clone(),
                });
            }
            Err(msg) => {
                if is_seed {
                    error!("failed to bindgen package", msg);
                    return Err(1);
                }
                print_warning(&format!("skipping transitive {}: {}", package_path, msg));
                continue;
            }
        }

        let imports = match workspace.imports_of(module, &package_path) {
            Ok(i) => i,
            Err(msg) => {
                print_warning(&format!(
                    "skipping import-walk for {}: {}",
                    package_path, msg
                ));
                continue;
            }
        };

        for import in imports {
            if deps::is_stdlib(&import) {
                continue;
            }
            let containing = match workspace.find_containing_module(&import) {
                Ok(info) if !info.path.is_empty() => info,
                _ => {
                    print_warning(&format!(
                        "could not resolve containing module for `{}` (referenced by {})",
                        import, package_path
                    ));
                    continue;
                }
            };
            if containing.path == module_path {
                let key = (containing.path, import);
                if !visited.contains(&key) {
                    queue.push((key.0, version.clone(), key.1));
                }
                continue;
            }

            // Record cache-walk-discovered modules so the manifest declares
            // every module whose typedef ends up in the cache.
            let next_version = if let Some(v) = module_graph.versions.get(&containing.path) {
                v.clone()
            } else {
                let resolved = if !containing.version.is_empty() {
                    containing.version
                } else {
                    match workspace.query_version(&containing.path) {
                        Ok(v) => v,
                        Err(msg) => {
                            print_warning(&format!("skipping transitive {}: {}", import, msg));
                            continue;
                        }
                    }
                };
                module_graph
                    .versions
                    .insert(containing.path.clone(), resolved.clone());
                module_graph
                    .edges
                    .entry(containing.path.clone())
                    .or_default();
                resolved
            };

            let parent_edges = module_graph.edges.entry(module_path.clone()).or_default();
            if !parent_edges.contains(&containing.path) {
                parent_edges.push(containing.path.clone());
            }

            let key = (containing.path.clone(), import.clone());
            if visited.contains(&key) {
                continue;
            }
            queue.push((containing.path, next_version, import));
        }
    }

    Ok(bindgenned)
}

struct BindgennedPackage {
    module: String,
    version: String,
    package: String,
}

/// Re-reconcile cache entries whose module version was raised by MVS drift.
fn rebuild_drifted_cache_entries(
    workspace: &GoWorkspace,
    graph: &GraphResult,
    bindgenned: &[BindgennedPackage],
) {
    for entry in bindgenned {
        let Some(current) = graph.versions.get(&entry.module) else {
            continue;
        };
        if current == &entry.version {
            continue;
        }
        let module = GoModule {
            path: &entry.module,
            version: current,
        };
        match workspace.reconcile_package(module, &entry.package) {
            Ok(stubs) => warn_stubbed(&stubs),
            Err(msg) => {
                print_warning(&format!(
                    "could not re-bindgen `{}` after MVS drift to {}: {}",
                    entry.package, current, msg
                ));
            }
        }
    }
}

fn warn_stubbed(stubs: &[String]) {
    for stubbed in stubs {
        print_warning(&format!(
            "{}: type-check failed; emitted as unloadable stub",
            stubbed
        ));
    }
}

/// Run the manifest walk for modules in `graph.versions` whose
/// `find_third_party_modules` result is missing, until the graph is closed
/// under MVS drift. Failures are warnings since these are all transitives.
fn expand_unwalked_modules(workspace: &GoWorkspace, graph: &mut GraphResult) -> Result<(), i32> {
    let mut failed: HashSet<String> = HashSet::new();

    let mut queue: Vec<String> = graph
        .versions
        .keys()
        .filter(|m| !graph.expanded.contains(*m))
        .cloned()
        .collect();

    loop {
        while let Some(module_path) = queue.pop() {
            if graph.expanded.contains(&module_path) {
                continue;
            }

            if !graph.versions.contains_key(&module_path) {
                match workspace.query_version(&module_path) {
                    Ok(v) => {
                        graph.versions.insert(module_path.clone(), v);
                    }
                    Err(msg) => {
                        if failed.insert(module_path.clone()) {
                            print_warning(&format!("skipping transitive {}: {}", module_path, msg));
                        }
                        continue;
                    }
                }
            }

            let listed = match workspace.find_third_party_modules(&module_path) {
                Ok(l) => l,
                Err(msg) => {
                    if failed.insert(module_path.clone()) {
                        print_warning(&format!("skipping transitive {}: {}", module_path, msg));
                    }
                    continue;
                }
            };

            for err in &listed.package_errors {
                print_warning(&format!(
                    "{}: package error in `{}`: {}",
                    module_path, err.package, err.message
                ));
            }

            let entry = graph.edges.entry(module_path.clone()).or_default();
            for next in &listed.modules {
                if !entry.contains(next) {
                    entry.push(next.clone());
                }
            }
            graph.expanded.insert(module_path);

            for next in listed.modules {
                if !graph.expanded.contains(&next) {
                    queue.push(next);
                }
            }
        }

        let drift = detect_mvs_drift(workspace, &graph.versions);
        for (module, msg) in drift.errors {
            if failed.insert(module.clone()) {
                print_warning(&format!(
                    "could not re-query version for {}: {}",
                    module, msg
                ));
            }
        }

        if drift.upgraded.is_empty() {
            break;
        }

        // Drifted module's outgoing edges may have changed; parent edges
        // pointing at it still stand (parent still imports it).
        for (module, new_version) in drift.upgraded {
            graph.versions.insert(module.clone(), new_version);
            graph.expanded.remove(&module);
            graph.edges.remove(&module);
            queue.push(module);
        }
    }

    Ok(())
}

/// Seed the cache walk's queue. Falls back to enumerating subpackages when
/// the requested module has no root package (e.g. `golang.org/x/sync`).
fn seed_cache_walk(
    canonical_module: &str,
    requested_package: &str,
    workspace: &GoWorkspace,
    queue: &mut Vec<(String, String, String)>,
) -> Result<HashSet<(String, String)>, i32> {
    let version = match workspace.query_version(canonical_module) {
        Ok(v) => v,
        Err(msg) => {
            error!("failed to resolve module version", msg);
            return Err(1);
        }
    };

    let push_seed = |queue: &mut Vec<_>, seeds: &mut HashSet<_>, package: String| {
        seeds.insert((canonical_module.to_string(), package.clone()));
        queue.push((canonical_module.to_string(), version.clone(), package));
    };

    let mut seeds: HashSet<(String, String)> = HashSet::new();

    if canonical_module != requested_package {
        push_seed(queue, &mut seeds, requested_package.to_string());
        return Ok(seeds);
    }

    let packages = match workspace.list_packages(canonical_module) {
        Ok(p) => p,
        Err(msg) => {
            error!("failed to list packages", msg);
            return Err(1);
        }
    };

    if packages.iter().any(|p| p == canonical_module) {
        push_seed(queue, &mut seeds, canonical_module.to_string());
        return Ok(seeds);
    }

    if packages.is_empty() {
        cli_error!(
            "Cannot bindgen module",
            format!("module `{}` has no importable packages", canonical_module),
            "Check the module path and try a specific subpackage like `lis add <module>/<sub>`"
        );
        return Err(1);
    }

    for pkg in packages {
        push_seed(queue, &mut seeds, pkg);
    }
    Ok(seeds)
}

#[derive(Default)]
struct DriftReport {
    /// `(module, new_version)` pairs whose pin moved.
    upgraded: Vec<(String, String)>,
    /// `(module, error)` pairs we could not re-query.
    errors: Vec<(String, String)>,
}

/// Snapshot every recorded module's pin and return the diff against Go's
/// current state.
fn detect_mvs_drift(workspace: &GoWorkspace, versions: &HashMap<String, String>) -> DriftReport {
    let mut report = DriftReport::default();
    let snapshot: Vec<(String, String)> = versions
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (module, recorded) in snapshot {
        match workspace.query_version(&module) {
            Ok(current) if current != recorded => report.upgraded.push((module, current)),
            Ok(_) => {}
            Err(msg) => report.errors.push((module, msg)),
        }
    }
    report
}

struct DirectUpgrade {
    path: String,
    old_version: String,
    new_version: String,
}

/// Update `lisette.toml` to reflect the newly reconciled `added_dep` subgraph,
/// leaving every other direct dep and its transitives untouched.
///
/// Four kinds of writes:
/// 1. `added_dep` itself - upsert with its final version
/// 2. Transitives reachable from `added_dep` - upsert with `via` entries
///    pointing back to their parents in the new graph
/// 3. Cleanup: for transitives in the old manifest that listed `added_dep` as a
///    parent but no longer appear in the new graph, strip `added_dep` from
///    their `via`; remove the entry entirely if nothing is left
/// 4. Hygiene: prune `via` entries that point to modules no longer present in
///    the manifest, and drop transitives left without any parent
///
/// Example of (3): before `lis add mux@newer`, the manifest has
/// `gorilla/context = { via = ["mux"] }`. The new mux version no longer imports
/// context, so context is no longer reachable from the added subgraph. `via`
/// becomes `[]`, and the entry is removed.
fn apply_graph_to_manifest(
    added_dep: &str,
    ctx: &ProjectContext,
    workspace: &GoWorkspace,
    graph: &GraphResult,
) -> Result<Vec<DirectUpgrade>, i32> {
    let project_root = &ctx.project_root;
    let existing_deps = ctx.manifest.go_deps();
    let transitives = graph.transitive_map(added_dep);
    let added_dep_version = graph
        .versions
        .get(added_dep)
        .map(|v| v.as_str())
        .unwrap_or(&ctx.resolved_version);
    let mut upgraded: Vec<DirectUpgrade> = Vec::new();

    if let Err(msg) = upsert_go_dep(project_root, added_dep, added_dep_version, None) {
        error!("failed to update manifest", msg);
        return Err(1);
    }

    let mut sorted_transitives: Vec<(&String, &Vec<String>)> = transitives.iter().collect();
    sorted_transitives.sort_by(|a, b| a.0.cmp(b.0));

    for (module_path, parents) in &sorted_transitives {
        let version = match graph.versions.get(module_path.as_str()) {
            Some(v) => v.as_str(),
            None => continue,
        };

        // If already a direct dep, refresh the version but keep it direct
        if let Some(existing) = existing_deps.get(module_path.as_str())
            && existing.via.is_none()
        {
            if existing.version != version {
                upsert_go_dep(project_root, module_path, version, None).map_err(|msg| {
                    error!("failed to update manifest", msg);
                    1
                })?;
                upgraded.push(DirectUpgrade {
                    path: (*module_path).clone(),
                    old_version: existing.version.clone(),
                    new_version: version.to_string(),
                });
            }
            continue;
        }

        let mut via: Vec<String> = existing_deps
            .get(module_path.as_str())
            .and_then(|d| d.via.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p != added_dep)
            .collect();

        for parent in parents.iter() {
            if !via.contains(parent) {
                via.push(parent.clone());
            }
        }
        via.sort();

        if let Err(msg) = upsert_go_dep(project_root, module_path, version, Some(via)) {
            error!("failed to update manifest", msg);
            return Err(1);
        }
    }

    let mut sorted_existing: Vec<(&String, &deps::GoDependency)> = existing_deps.iter().collect();
    sorted_existing.sort_by(|a, b| a.0.cmp(b.0));

    for (dep_path, dep) in &sorted_existing {
        if transitives.contains_key(dep_path.as_str()) {
            continue;
        }

        let Some(ref old_via) = dep.via else { continue };

        if !old_via.iter().any(|p| p == added_dep) {
            continue;
        }

        let mut filtered: Vec<String> = old_via
            .iter()
            .filter(|p| *p != added_dep)
            .cloned()
            .collect();
        filtered.sort();

        if filtered.is_empty() {
            remove_go_dep(project_root, dep_path).map_err(|msg| {
                error!("failed to update manifest", msg);
                1
            })?;
            continue;
        }

        let dep_version = workspace.query_version(dep_path).map_err(|msg| {
            error!("failed to resolve module version", msg);
            1
        })?;

        upsert_go_dep(project_root, dep_path, &dep_version, Some(filtered)).map_err(|msg| {
            error!("failed to update manifest", msg);
            1
        })?;
    }

    trim_dead_via_parents(project_root).map_err(|msg| {
        error!("failed to update manifest", msg);
        1
    })?;
    resolve_empty_via(project_root, &[]).map_err(|msg| {
        error!("failed to update manifest", msg);
        1
    })?;

    Ok(upgraded)
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
