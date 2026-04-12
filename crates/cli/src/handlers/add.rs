use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::go_cli;
use crate::output::{print_add_success, print_preview_notice, print_progress};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::{GoModule, remove_go_dep, upsert_go_dep};

struct ParsedDependency {
    module_path: String,
    version: String,
}

struct ProjectContext {
    project_root: PathBuf,
    target_dir: PathBuf,
    manifest: deps::Manifest,
    typedef_cache_dir: PathBuf,
    resolved_version: String,
    _lock: File,
}

struct GraphResult {
    /// Final MVS-selected version for each reconciled module, e.g.
    /// `{ "github.com/gorilla/mux" → "v1.8.1" }`.
    versions: HashMap<String, String>,
    /// For each reconciled module, the third-party modules it imports
    /// via its typedefs, e.g. `{ "mux" → ["context"] }`.
    edges: HashMap<String, Vec<String>>,
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

    let dep = match parse_dep_string(dep_string) {
        Ok(dep) => dep,
        Err(msg) => {
            cli_error!(
                "Invalid dependency",
                msg,
                "Example: `lis add github.com/gorilla/mux@v1.8.0`"
            );
            return 1;
        }
    };

    let project_ctx = match setup_project(&dep) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let workspace = GoWorkspace::new(&project_ctx.target_dir, &project_ctx.typedef_cache_dir);

    let module_graph = match reconcile_module_graph(&dep, &workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let upgraded =
        match apply_graph_to_manifest(&dep.module_path, &project_ctx, &workspace, &module_graph) {
            Ok(u) => u,
            Err(code) => return code,
        };

    let dep_version = module_graph
        .versions
        .get(&dep.module_path)
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
        &dep.module_path,
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

    if stdlib::get_go_stdlib_typedef(path).is_some() {
        return Err(format!(
            "`{}` is a Go standard library package; stdlib packages do not need `lis add` (just `import \"go:{}\"`)",
            path, path
        ));
    }

    let module_path = if deps::is_third_party(path) {
        path.to_string()
    } else if path.contains('/') {
        format!("github.com/{}", path)
    } else {
        return Err(format!(
            "`{}` is not a valid module path; expected something like `github.com/owner/repo`",
            path
        ));
    };

    if module_path == PRELUDE_MODULE {
        return Err(
            "the Lisette prelude is built into every project and cannot be added as a dependency"
                .to_string(),
        );
    }

    let version = if version == "latest" {
        version
    } else if version.starts_with('v') || version.starts_with('V') {
        format!("v{}", &version[1..])
    } else if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("v{}", version)
    } else {
        // Pass through branch names, commit hashes, HEAD, etc. unchanged so
        // `go get` can resolve them to pseudo-versions.
        version
    };

    Ok(ParsedDependency {
        module_path,
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

fn find_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut current: &Path = &cwd;
    loop {
        if current.join("lisette.toml").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn setup_project(dep: &ParsedDependency) -> Result<ProjectContext, i32> {
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

    let lock = acquire_add_lock(&project_target_dir)?;

    let locator = deps::TypedefLocator::new(
        manifest.go_deps(),
        Some(project_root.clone()),
        std::env::var("HOME").ok(),
    );

    if let Err(msg) = go_cli::write_go_mod(&project_target_dir, &manifest.project.name, &locator) {
        error!("failed to write target/go.mod", msg);
        return Err(1);
    }

    let typedef_cache_dir = match std::env::var("HOME") {
        Ok(h) => deps::typedef_cache_dir(&h),
        Err(_) => {
            error!(
                "failed to add dependency",
                "HOME environment variable not set".to_string()
            );
            return Err(1);
        }
    };

    let workspace = GoWorkspace::new(&project_target_dir, &typedef_cache_dir);

    let dep_version = if dep.version == "latest" {
        print_progress(&format!("Resolving {}@latest", dep.module_path));
        match workspace.query_latest_version(&dep.module_path) {
            Ok(v) => v,
            Err(msg) => {
                error!("failed to resolve latest version", msg);
                return Err(1);
            }
        }
    } else {
        dep.version.clone()
    };

    print_progress(&format!("Fetching {}@{}", dep.module_path, dep_version));

    if let Err(msg) = workspace.go_get(GoModule {
        path: &dep.module_path,
        version: &dep_version,
    }) {
        error!("failed to download dependency", msg);
        return Err(1);
    }

    Ok(ProjectContext {
        project_root,
        target_dir: project_target_dir,
        manifest,
        typedef_cache_dir,
        resolved_version: dep_version,
        _lock: lock,
    })
}

fn acquire_add_lock(target_dir: &Path) -> Result<File, i32> {
    let lock_path = target_dir.join(".lis-add.lock");
    let file = match File::create(&lock_path) {
        Ok(f) => f,
        Err(e) => {
            error!(
                "failed to create lock file",
                format!("Failed to create `{}`: {}", lock_path.display(), e)
            );
            return Err(1);
        }
    };

    if let Err(e) = file.try_lock_exclusive() {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            cli_error!(
                "Another `lis add` is in progress",
                "A concurrent `lis add` holds the project lock",
                "Wait for the other invocation to finish, then retry"
            );
        } else {
            error!("failed to acquire lock", format!("{}", e));
        }
        return Err(1);
    }

    Ok(file)
}

/// Walk the dependency tree reachable from `dep` and cache typedefs for every
/// module at its final MVS-selected version.
///
/// BFS-discovers modules by scanning each reconciled module's typedefs for
/// `import "go:..."` references. Because Go's MVS can upgrade an
/// already-reconciled module when a later `go get` raises its version, a drift
/// fixup pass re-queries every reconciled module after the BFS drains and
/// re-enqueues any that shifted. MVS only moves upward, so this converges.
///
/// Example: `lis add gorilla/mux` reconciles `mux`, finds it imports
/// `gorilla/context`, reconciles `context`. Returns:
///
/// ```text
/// module_versions: { mux → v1.8.1, context → v1.1.1 }
/// edges:           { mux → [context], context → [] }
/// ```
fn reconcile_module_graph(
    dep: &ParsedDependency,
    workspace: &GoWorkspace,
) -> Result<GraphResult, i32> {
    let mut module_versions: HashMap<String, String> = HashMap::new();
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut queue: Vec<String> = vec![dep.module_path.clone()];

    loop {
        while let Some(module_path) = queue.pop() {
            let module_version = workspace.query_version(&module_path).map_err(|msg| {
                error!("failed to resolve module version", msg);
                1
            })?;

            if module_versions
                .get(&module_path)
                .is_some_and(|v| *v == module_version)
            {
                continue;
            }

            if module_path != dep.module_path && !module_versions.contains_key(&module_path) {
                print_progress(&format!("Resolving transitive dep {}", module_path));
            }

            let module = GoModule {
                path: &module_path,
                version: &module_version,
            };

            let packages = match workspace.reconcile(module) {
                Ok(p) => p,
                Err(msg) => {
                    error!("failed to reconcile dependency", msg);
                    return Err(1);
                }
            };

            let dep_modules = match workspace.find_third_party_deps(module, &packages) {
                Ok(t) => t,
                Err(msg) => {
                    error!("failed to scan transitive imports", msg);
                    return Err(1);
                }
            };

            module_versions.insert(module_path.clone(), module_version);
            edges.insert(module_path, dep_modules.clone());

            for dep_module in dep_modules {
                queue.push(dep_module);
            }
        }

        // Check if MVS upgraded any module since it was reconciled
        let mut more_work = false;
        let snapshot: Vec<_> = module_versions
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (module, reconciled_version) in snapshot {
            let current_version = workspace.query_version(&module).map_err(|msg| {
                error!("failed to resolve module version", msg);
                1
            })?;
            if current_version != reconciled_version {
                queue.push(module);
                more_work = true;
            }
        }

        if !more_work {
            break;
        }
    }

    Ok(GraphResult {
        versions: module_versions,
        edges,
    })
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
        .unwrap_or("");
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

    prune_stale_via(project_root)?;

    Ok(upgraded)
}

/// Drop `via` entries that point to parents no longer present in the manifest,
/// and remove transitives left with an empty `via` list.
fn prune_stale_via(project_root: &Path) -> Result<(), i32> {
    let manifest = match deps::parse_manifest(project_root) {
        Ok(m) => m,
        Err(msg) => {
            error!("failed to re-read manifest for hygiene pass", msg);
            return Err(1);
        }
    };
    let live_deps = manifest.go_deps();
    let live_paths: std::collections::HashSet<&str> =
        live_deps.keys().map(|s| s.as_str()).collect();

    let mut sorted: Vec<(&String, &deps::GoDependency)> = live_deps.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    for (dep_path, dep) in &sorted {
        let Some(ref via) = dep.via else { continue };

        let mut canonical: Vec<String> = via
            .iter()
            .filter(|parent| live_paths.contains(parent.as_str()))
            .cloned()
            .collect();
        canonical.sort();
        canonical.dedup();

        if canonical.iter().eq(via.iter()) {
            continue;
        }

        if canonical.is_empty() {
            remove_go_dep(project_root, dep_path).map_err(|msg| {
                error!("failed to update manifest", msg);
                1
            })?;
            continue;
        }

        upsert_go_dep(project_root, dep_path, &dep.version, Some(canonical)).map_err(|msg| {
            error!("failed to update manifest", msg);
            1
        })?;
    }

    Ok(())
}
