//! Go dependency reconciliation engine.
//!
//! The graph walk (bindgen each package, follow its imports), MVS-drift
//! convergence, and manifest application live here, separate from the `lis add`
//! CLI handler that drives them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::output::{print_progress, print_warning};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::{GoModule, remove_go_dep, resolve_empty_via, trim_dead_via_parents, upsert_go_dep};

/// The dependency to reconcile, after its containing module is resolved.
pub(crate) struct ResolvedDependency {
    pub(crate) requested_package: String,
    pub(crate) canonical_module: String,
}

pub(crate) struct GraphResult {
    /// Final MVS-selected version for each reconciled module, e.g.
    /// `{ "github.com/gorilla/mux" → "v1.8.1" }`.
    pub(crate) versions: HashMap<String, String>,
    /// For each reconciled module, the third-party modules it imports
    /// via its typedefs, e.g. `{ "mux" → ["context"] }`.
    pub(crate) edges: HashMap<String, Vec<String>>,
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

pub(crate) fn reconcile_module_graph(
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
pub(crate) fn walk_typedef_cache(
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

pub(crate) struct BindgennedPackage {
    module: String,
    version: String,
    package: String,
}

/// Re-reconcile cache entries whose module version was raised by MVS drift.
pub(crate) fn rebuild_drifted_cache_entries(
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
pub(crate) fn expand_unwalked_modules(
    workspace: &GoWorkspace,
    graph: &mut GraphResult,
) -> Result<(), i32> {
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

pub(crate) struct DirectUpgrade {
    pub(crate) path: String,
    pub(crate) old_version: String,
    pub(crate) new_version: String,
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
pub(crate) fn apply_graph_to_manifest(
    added_dep: &str,
    project_root: &Path,
    manifest: &deps::Manifest,
    fallback_version: &str,
    workspace: &GoWorkspace,
    graph: &GraphResult,
) -> Result<Vec<DirectUpgrade>, i32> {
    let existing_deps = manifest.go_deps();
    let transitives = graph.transitive_map(added_dep);
    let added_dep_version = graph
        .versions
        .get(added_dep)
        .map(|v| v.as_str())
        .unwrap_or(fallback_version);
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
