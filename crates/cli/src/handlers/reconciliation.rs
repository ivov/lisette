//! Go dependency reconciliation engine, shared by `lis add` and `lis sync`.
//!
//! The graph walk (bindgen each package, follow its imports), MVS-drift
//! convergence, replacement-closure reconciliation, and manifest application live here
//! so neither handler owns them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::go_cli;
use crate::output::{print_progress, print_warning};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::{GoModule, resolve_empty_via, trim_dead_via_parents, upsert_go_dependency};
use stdlib::Target;

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

/// The replacement a `--replace` add resolves to: where its code comes from.
#[derive(Clone)]
pub(crate) struct ReplacementIdentity {
    pub(crate) replacement_path: String,
    pub(crate) replacement_version: String,
}

/// How `apply_graph_to_manifest` writes a replaced root: `AddDirect` promotes it to
/// a direct dep, `SyncPreserveVia` keeps an existing `via` (so a replaced
/// transitive is not silently promoted).
pub(crate) enum ReplacedRootMode {
    AddDirect,
    SyncPreserveVia,
}

pub(crate) struct ReplacedRoot<'a> {
    pub(crate) identity: &'a ReplacementIdentity,
    pub(crate) mode: ReplacedRootMode,
}

/// Re-walk every declared replacement's closure and reconcile the manifest, for `lis sync`.
pub(crate) fn reconcile_declared_replacements(
    project_root: &Path,
    target_dir: &Path,
    manifest: &deps::Manifest,
) -> Result<(), i32> {
    let replaced_roots: Vec<(String, ReplacementIdentity)> = manifest
        .go_deps()
        .into_iter()
        .filter_map(|(module, dep)| match dep {
            deps::GoDependency::Replaced {
                replacement_path,
                replacement_version,
                ..
            } => Some((
                module,
                ReplacementIdentity {
                    replacement_path,
                    replacement_version,
                },
            )),
            deps::GoDependency::Remote { .. } => None,
        })
        .collect();

    if replaced_roots.is_empty() {
        return Ok(());
    }

    go_cli::require_go()?;

    // Seed every declared dep so MVS picks the versions the real build sees.
    let locator = deps::TypedefLocator::new(
        manifest.go_deps(),
        Some(project_root.to_path_buf()),
        Target::host(),
    );
    if let Err(msg) = go_cli::write_go_mod(target_dir, &manifest.project.name, &locator) {
        error!("failed to write target/go.mod", msg);
        return Err(1);
    }

    let typedef_cache_dir = deps::typedef_cache_dir(project_root);
    let workspace = GoWorkspace::new(target_dir, &typedef_cache_dir, Target::host());

    // Walk all replacements before writing the manifest, so a partial failure leaves it untouched.
    let replacements: HashMap<String, ReplacementIdentity> =
        replaced_roots.iter().cloned().collect();

    let mut walked: Vec<(String, ReplacementIdentity, GraphResult)> = Vec::new();
    for (original, replacement) in &replaced_roots {
        let resolved = ResolvedDependency {
            requested_package: original.clone(),
            canonical_module: original.clone(),
        };

        let graph = reconcile_root(&resolved, &workspace, &replacements)?;
        walked.push((original.clone(), replacement.clone(), graph));
    }

    for (original, replacement, graph) in &walked {
        let current = match deps::parse_manifest(project_root) {
            Ok(m) => m,
            Err(msg) => {
                error!("failed to read manifest", msg);
                return Err(1);
            }
        };
        apply_graph_to_manifest(
            original,
            project_root,
            &current,
            &replacement.replacement_version,
            &workspace,
            graph,
            Some(ReplacedRoot {
                identity: replacement,
                mode: ReplacedRootMode::SyncPreserveVia,
            }),
        )?;
    }

    Ok(())
}

/// A Go resolution error that means a `replace` target is not import-compatible:
/// its own packages do not resolve under the original module path (Go binds one
/// module to two import paths).
fn import_compat_hint(go_error: &str) -> Option<&'static str> {
    go_error
        .contains("used for two different module paths")
        .then_some(
            "the `replace` target is not import-compatible: keep its `module` line as the original module path so its own imports resolve",
        )
}

/// Reconcile a root's full module graph: walk the manifest subgraph, build each
/// package's typedefs, expand modules the walk did not reach, and rebuild any
/// cache entry MVS drift moved to a new version. Returns the resolved graph for
/// `apply_graph_to_manifest`, shared by `lis add` and `lis sync`.
pub(crate) fn reconcile_root(
    dep: &ResolvedDependency,
    workspace: &GoWorkspace,
    replacements: &HashMap<String, ReplacementIdentity>,
) -> Result<GraphResult, i32> {
    let mut graph = reconcile_module_graph(dep, workspace)?;
    let bindgenned = walk_typedef_cache(dep, workspace, &mut graph, replacements)?;
    expand_unwalked_modules(workspace, &mut graph)?;
    rebuild_drifted_cache_entries(workspace, &graph, &bindgenned, replacements);
    Ok(graph)
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
                        match import_compat_hint(&msg) {
                            Some(hint) => {
                                cli_error!("failed to resolve module version", msg, hint)
                            }
                            None => error!("failed to resolve module version", msg),
                        }
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
                        match import_compat_hint(&msg) {
                            Some(hint) => {
                                cli_error!("failed to scan transitive modules", msg, hint)
                            }
                            None => error!("failed to scan transitive modules", msg),
                        }
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

/// The `Replacement` a module's typedef cache is keyed by, if it is a declared replacement.
fn replacement_for<'a>(
    module_path: &str,
    replacements: &'a HashMap<String, ReplacementIdentity>,
) -> Option<deps::Replacement<'a>> {
    replacements
        .get(module_path)
        .map(|identity| deps::Replacement {
            path: &identity.replacement_path,
            version: &identity.replacement_version,
        })
}

/// The declared replacement redirects, keyed by original module path.
pub(crate) fn declared_replacements(
    manifest: &deps::Manifest,
) -> HashMap<String, ReplacementIdentity> {
    manifest
        .go_deps()
        .into_iter()
        .filter_map(|(module, dep)| match dep {
            deps::GoDependency::Replaced {
                replacement_path,
                replacement_version,
                ..
            } => Some((
                module,
                ReplacementIdentity {
                    replacement_path,
                    replacement_version,
                },
            )),
            deps::GoDependency::Remote { .. } => None,
        })
        .collect()
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
    replacements: &HashMap<String, ReplacementIdentity>,
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
            replacement: replacement_for(&module_path, replacements),
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
fn rebuild_drifted_cache_entries(
    workspace: &GoWorkspace,
    graph: &GraphResult,
    bindgenned: &[BindgennedPackage],
    replacements: &HashMap<String, ReplacementIdentity>,
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
            replacement: replacement_for(&entry.module, replacements),
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
    replaced_root: Option<ReplacedRoot>,
) -> Result<Vec<DirectUpgrade>, i32> {
    let existing_deps = manifest.go_deps();
    let transitives = graph.transitive_map(added_dep);
    let added_dep_version = graph
        .versions
        .get(added_dep)
        .map(|v| v.as_str())
        .unwrap_or(fallback_version);
    let mut upgraded: Vec<DirectUpgrade> = Vec::new();

    let root_result = match replaced_root {
        Some(replaced_root) => {
            let via = match replaced_root.mode {
                ReplacedRootMode::SyncPreserveVia => existing_deps
                    .get(added_dep)
                    .and_then(|d| d.via().map(<[String]>::to_vec)),
                ReplacedRootMode::AddDirect => None,
            };
            upsert_go_dependency(
                project_root,
                added_dep,
                &deps::GoDependency::Replaced {
                    replacement_path: replaced_root.identity.replacement_path.clone(),
                    replacement_version: replaced_root.identity.replacement_version.clone(),
                    via,
                },
            )
        }
        None => upsert_go_dependency(
            project_root,
            added_dep,
            &deps::GoDependency::Remote {
                version: added_dep_version.to_string(),
                via: None,
            },
        ),
    };
    if let Err(msg) = root_result {
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

        // If already a direct dep, refresh the version but keep it direct.
        let existing = existing_deps.get(module_path.as_str());
        if let Some(existing) = existing
            && existing.via().is_none()
        {
            if let deps::GoDependency::Remote {
                version: existing_version,
                ..
            } = existing
                && existing_version != version
            {
                upsert_go_dependency(
                    project_root,
                    module_path,
                    &deps::GoDependency::Remote {
                        version: version.to_string(),
                        via: None,
                    },
                )
                .map_err(|msg| {
                    error!("failed to update manifest", msg);
                    1
                })?;
                upgraded.push(DirectUpgrade {
                    path: (*module_path).clone(),
                    old_version: existing_version.clone(),
                    new_version: version.to_string(),
                });
            }
            continue;
        }

        let mut via: Vec<String> = existing
            .and_then(|d| d.via().map(<[String]>::to_vec))
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

        // A replaced transitive keeps its `replace` shape, only its `via` is reconciled.
        let result = match existing {
            Some(replaced @ deps::GoDependency::Replaced { .. }) => {
                upsert_go_dependency(project_root, module_path, &replaced.with_via(Some(via)))
            }
            _ => upsert_go_dependency(
                project_root,
                module_path,
                &deps::GoDependency::Remote {
                    version: version.to_string(),
                    via: Some(via),
                },
            ),
        };
        if let Err(msg) = result {
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

        let Some(old_via) = dep.via() else { continue };

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
            upsert_go_dependency(project_root, dep_path, &dep.with_via(Some(Vec::new()))).map_err(
                |msg| {
                    error!("failed to update manifest", msg);
                    1
                },
            )?;
            continue;
        }

        match dep {
            deps::GoDependency::Replaced { .. } => {
                upsert_go_dependency(project_root, dep_path, &dep.with_via(Some(filtered)))
                    .map_err(|msg| {
                        error!("failed to update manifest", msg);
                        1
                    })?;
            }
            deps::GoDependency::Remote { .. } => {
                let dep_version = workspace.query_version(dep_path).map_err(|msg| {
                    error!("failed to resolve module version", msg);
                    1
                })?;
                upsert_go_dependency(
                    project_root,
                    dep_path,
                    &deps::GoDependency::Remote {
                        version: dep_version,
                        via: Some(filtered),
                    },
                )
                .map_err(|msg| {
                    error!("failed to update manifest", msg);
                    1
                })?;
            }
        }
    }

    Ok(upgraded)
}

/// Trim dead `via` parents and promote/drop empty-`via` entries, to a fixed
/// point (a removal can orphan another's `via`). `imported_pkgs` promotes a
/// directly-imported transitive instead of deleting it.
pub(crate) fn finalize_manifest_via(
    project_root: &Path,
    imported_pkgs: &[String],
) -> Result<(Vec<deps::TrimmedVia>, deps::ResolveReport), i32> {
    let mut all_trimmed = Vec::new();
    let mut promoted = Vec::new();
    let mut removed = Vec::new();

    loop {
        let trimmed = trim_dead_via_parents(project_root).map_err(|msg| {
            error!("failed to update manifest", msg);
            1
        })?;
        let report = resolve_empty_via(project_root, imported_pkgs).map_err(|msg| {
            error!("failed to update manifest", msg);
            1
        })?;

        let changed =
            !trimmed.is_empty() || !report.promoted.is_empty() || !report.removed.is_empty();
        all_trimmed.extend(trimmed);
        promoted.extend(report.promoted);
        removed.extend(report.removed);

        if !changed {
            break;
        }
    }

    Ok((all_trimmed, deps::ResolveReport { promoted, removed }))
}
