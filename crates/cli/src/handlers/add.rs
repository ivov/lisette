use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    print_preview_notice();

    let project_ctx = match setup_project(&dep) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let workspace = GoWorkspace::new(&project_ctx.target_dir, &project_ctx.typedef_cache_dir);

    let module_graph = match reconcile_module_graph(&dep, &workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if let Err(code) =
        apply_graph_to_manifest(&dep.module_path, &project_ctx, &workspace, &module_graph)
    {
        return code;
    }

    let dep_version = module_graph
        .versions
        .get(&dep.module_path)
        .cloned()
        .unwrap_or(project_ctx.resolved_version);

    print_add_success(
        &dep.module_path,
        &dep_version,
        &module_graph.edges,
        &module_graph.versions,
    );

    0
}

fn parse_dep_string(input: &str) -> Result<ParsedDependency, String> {
    let (path, version) = match input.rsplit_once('@') {
        Some((p, v)) if !p.is_empty() && !v.is_empty() => (p, v.to_string()),
        None if !input.is_empty() => (input, "latest".to_string()),
        _ => return Err(format!("Cannot parse `{}`", input)),
    };

    let module_path = if !deps::is_third_party(path) {
        format!("github.com/{}", path)
    } else {
        path.to_string()
    };

    let version = if version == "latest" {
        version
    } else if !version.starts_with('v') {
        format!("v{}", version)
    } else {
        version
    };

    Ok(ParsedDependency {
        module_path,
        version,
    })
}

fn setup_project(dep: &ParsedDependency) -> Result<ProjectContext, i32> {
    let project_root = Path::new(".");
    if !project_root.join("lisette.toml").exists() {
        cli_error!(
            "No project found",
            "No `lisette.toml` in current directory",
            "Run `lis new <name>` to create a project"
        );
        return Err(1);
    }

    let manifest = match deps::parse_manifest(project_root) {
        Ok(m) => m,
        Err(msg) => {
            cli_error!("Failed to read manifest", msg, "Fix `lisette.toml`");
            return Err(1);
        }
    };

    if let Err(msg) = deps::check_toolchain_version(&manifest) {
        error!("toolchain mismatch", msg);
        return Err(1);
    }

    let project_target_dir = project_root.join("target");
    if let Err(e) = std::fs::create_dir_all(&project_target_dir) {
        error!(
            "failed to set up target directory",
            format!("Failed to create target directory: {}", e)
        );
        return Err(1);
    }

    let locator = deps::TypedefLocator::new(
        manifest.go_deps(),
        Some(project_root.to_path_buf()),
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
        let query = format!("{}@latest", dep.module_path);
        match workspace.query_module(&query) {
            Ok(info) => info.version,
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
        project_root: project_root.to_path_buf(),
        target_dir: project_target_dir,
        manifest,
        typedef_cache_dir,
        resolved_version: dep_version,
    })
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

/// Update `lisette.toml` to reflect the newly reconciled `added_dep` subgraph,
/// leaving every other direct dep and its transitives untouched.
///
/// Three kinds of writes:
/// 1. `added_dep` itself - upsert with its final version
/// 2. Transitives reachable from `added_dep` - upsert with `via` entries
///    pointing back to their parents in the new graph
/// 3. Cleanup: for transitives in the old manifest that listed `added_dep` as a
///    parent but no longer appear in the new graph, strip `added_dep` from
///    their `via`; remove the entry entirely if nothing is left
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
) -> Result<(), i32> {
    let project_root = &ctx.project_root;
    let existing_deps = ctx.manifest.go_deps();
    let transitives = graph.transitive_map(added_dep);
    let added_dep_version = graph
        .versions
        .get(added_dep)
        .map(|v| v.as_str())
        .unwrap_or("");

    if let Err(msg) = upsert_go_dep(project_root, added_dep, added_dep_version, None) {
        error!("failed to update manifest", msg);
        return Err(1);
    }

    for (module_path, parents) in &transitives {
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

        for parent in parents {
            if !via.contains(parent) {
                via.push(parent.clone());
            }
        }

        if let Err(msg) = upsert_go_dep(project_root, module_path, version, Some(via)) {
            error!("failed to update manifest", msg);
            return Err(1);
        }
    }

    for (dep_path, dep) in &existing_deps {
        if transitives.contains_key(dep_path.as_str()) {
            continue;
        }

        let Some(ref old_via) = dep.via else { continue };

        if !old_via.iter().any(|p| p == added_dep) {
            continue;
        }

        let filtered: Vec<String> = old_via
            .iter()
            .filter(|p| *p != added_dep)
            .cloned()
            .collect();

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

    Ok(())
}
