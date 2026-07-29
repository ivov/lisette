//! Go dependency reconciliation engine, shared by `lis add` and `lis sync`.
//!
//! The graph walk (bindgen each package, follow its imports), MVS-drift
//! convergence, replacement-closure reconciliation, and manifest application live here
//! so neither handler owns them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::go_cli;
use crate::output::{print_progress, print_warning};
use crate::workspace::{GoWorkspace, UnresolvedTransitive};
use crate::{cli_error, error};
use deps::{GoModule, resolve_empty_via, trim_dead_via_parents, upsert_go_dependency};
use stdlib::Target;

/// The dependency to reconcile, after its containing module is resolved.
pub(crate) struct ResolvedDependency {
    pub(crate) requested_package: String,
    pub(crate) canonical_module: String,
}

#[derive(Default)]
pub(crate) struct GraphResult {
    modules: HashMap<String, ModuleState>,
}

enum ModuleState {
    /// Known through a typedef import, but not yet exhaustively scanned.
    Discovered {
        version: String,
        known_dependencies: Vec<String>,
    },
    /// Scanned at this version. An empty dependency list is a real leaf.
    Expanded {
        version: String,
        dependencies: Vec<String>,
    },
}

impl ModuleState {
    fn version(&self) -> &str {
        match self {
            Self::Discovered { version, .. } | Self::Expanded { version, .. } => version,
        }
    }

    fn dependencies(&self) -> &[String] {
        match self {
            Self::Discovered {
                known_dependencies, ..
            } => known_dependencies,
            Self::Expanded { dependencies, .. } => dependencies,
        }
    }

    fn dependencies_mut(&mut self) -> &mut Vec<String> {
        match self {
            Self::Discovered {
                known_dependencies, ..
            } => known_dependencies,
            Self::Expanded { dependencies, .. } => dependencies,
        }
    }

    fn is_expanded(&self) -> bool {
        matches!(self, Self::Expanded { .. })
    }
}

impl GraphResult {
    pub(crate) fn version(&self, module: &str) -> Option<&str> {
        self.modules.get(module).map(ModuleState::version)
    }

    fn is_expanded(&self, module: &str) -> bool {
        self.modules
            .get(module)
            .is_some_and(ModuleState::is_expanded)
    }

    fn discover(&mut self, module: String, version: String) {
        self.modules
            .entry(module)
            .or_insert_with(|| ModuleState::Discovered {
                version,
                known_dependencies: Vec::new(),
            });
    }

    fn expand(&mut self, module: String, version: String, mut dependencies: Vec<String>) {
        if let Some(ModuleState::Discovered {
            known_dependencies, ..
        }) = self.modules.get(&module)
        {
            for dependency in known_dependencies {
                if !dependencies.contains(dependency) {
                    dependencies.push(dependency.clone());
                }
            }
        }
        self.modules.insert(
            module,
            ModuleState::Expanded {
                version,
                dependencies,
            },
        );
    }

    fn rediscover(&mut self, module: String, version: String) {
        self.modules.insert(
            module,
            ModuleState::Discovered {
                version,
                known_dependencies: Vec::new(),
            },
        );
    }

    fn record_dependency(&mut self, parent: String, version: String, dependency: String) {
        let state = self
            .modules
            .entry(parent)
            .or_insert_with(|| ModuleState::Discovered {
                version,
                known_dependencies: Vec::new(),
            });
        let dependencies = state.dependencies_mut();
        if !dependencies.contains(&dependency) {
            dependencies.push(dependency);
        }
    }

    fn discovered_modules(&self) -> Vec<String> {
        self.modules
            .iter()
            .filter(|(_, state)| !state.is_expanded())
            .map(|(module, _)| module.clone())
            .collect()
    }

    pub(crate) fn versions(&self) -> HashMap<String, String> {
        self.modules
            .iter()
            .map(|(module, state)| (module.clone(), state.version().to_string()))
            .collect()
    }

    pub(crate) fn edges(&self) -> HashMap<String, Vec<String>> {
        self.modules
            .iter()
            .map(|(module, state)| (module.clone(), state.dependencies().to_vec()))
            .collect()
    }

    /// Invert `edges` into a `module → parents` map, excluding the added root.
    fn transitive_map(&self, added_module: &str) -> HashMap<String, Vec<String>> {
        let mut transitives: HashMap<String, Vec<String>> = HashMap::new();
        for (parent, state) in &self.modules {
            for child in state.dependencies() {
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

/// How `apply_graph_to_manifest` writes a replaced root: `AddDirect` promotes it to
/// a direct dep, `SyncPreserveVia` keeps an existing `via` (so a replaced
/// transitive is not silently promoted).
pub(crate) enum ReplacedRootMode {
    AddDirect,
    SyncPreserveVia,
}

pub(crate) struct ReplacedRoot<'a> {
    pub(crate) identity: &'a deps::ReplacementSource,
    pub(crate) mode: ReplacedRootMode,
}

pub(crate) enum RootWrite<'a> {
    Remote { fallback_version: &'a str },
    Replaced(ReplacedRoot<'a>),
}

/// Re-walk every declared replacement's closure and reconcile the manifest, for `lis sync`.
pub(crate) fn reconcile_declared_replacements(
    project_root: &Path,
    target_dir: &Path,
    manifest: &deps::Manifest,
) -> Result<(), i32> {
    let replaced_roots: Vec<(String, deps::ReplacementSource)> = manifest
        .go_deps()
        .into_iter()
        .filter_map(|(module, dep)| match dep {
            deps::GoDependency::Replaced { source, .. } => Some((module, source)),
            deps::GoDependency::Remote { .. } => None,
        })
        .collect();

    if replaced_roots.is_empty() {
        return Ok(());
    }

    // Seed every declared dep so MVS picks the versions the real build sees.
    go_cli::invalidate_go_mod_stamp(target_dir);
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
    let replacements: HashMap<String, deps::ReplacementSource> =
        replaced_roots.iter().cloned().collect();

    let locals = declared_local_dirs(&replacements, project_root);

    let mut walked: Vec<(String, deps::ReplacementSource, GraphResult)> = Vec::new();
    for (original, replacement) in &replaced_roots {
        let resolved = ResolvedDependency {
            requested_package: original.clone(),
            canonical_module: original.clone(),
        };

        let graph = reconcile_root(&resolved, &workspace, &replacements, &locals)?;
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
            &workspace,
            graph,
            RootWrite::Replaced(ReplacedRoot {
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
    replacements: &HashMap<String, deps::ReplacementSource>,
    locals: &[(String, PathBuf)],
) -> Result<GraphResult, i32> {
    let mut graph = reconcile_module_graph(dep, workspace, locals)?;
    let bindgenned = walk_typedef_cache(dep, workspace, &mut graph, replacements)?;
    expand_unwalked_modules(workspace, &mut graph, locals)?;
    rebuild_drifted_cache_entries(workspace, &graph, &bindgenned, replacements);
    Ok(graph)
}

fn handle_module_failure(
    context: &str,
    module_path: &str,
    message: &str,
    is_explicit: bool,
    failed_transitives: &mut HashSet<String>,
) -> Result<(), i32> {
    if is_explicit {
        match import_compat_hint(message) {
            Some(hint) => cli_error!(context, message, hint),
            None => error!(context, message),
        }
        return Err(1);
    }
    if failed_transitives.insert(module_path.to_string()) {
        print_warning(&format!("skipping transitive {}: {}", module_path, message));
    }
    Ok(())
}

/// Manifest walk: BFS the third-party module subgraph from `dep.canonical_module`
/// via `go list -json M/...`. Module-grained so the manifest declares every
/// module a future subpackage import could reach; the outer loop converges
/// MVS drift since MVS only moves upward.
fn reconcile_module_graph(
    dep: &ResolvedDependency,
    workspace: &GoWorkspace,
    locals: &[(String, PathBuf)],
) -> Result<GraphResult, i32> {
    let canonical_module = dep.canonical_module.as_str();

    let mut graph = GraphResult::default();
    let mut failed_transitives: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = vec![canonical_module.to_string()];

    loop {
        while let Some(module_path) = queue.pop() {
            let is_explicit = module_path == canonical_module;

            let module_version = match workspace.query_version(&module_path) {
                Ok(version) => version,
                Err(message) => {
                    handle_module_failure(
                        "failed to resolve module version",
                        &module_path,
                        &message,
                        is_explicit,
                        &mut failed_transitives,
                    )?;
                    continue;
                }
            };

            if graph.version(&module_path) == Some(&module_version) {
                continue;
            }

            if !is_explicit && graph.version(&module_path).is_none() {
                print_progress(&format!("Resolving transitive dep {}", module_path));
            }

            let listed = match workspace.find_third_party_modules(&module_path) {
                Ok(listed) => listed,
                Err(message) => {
                    handle_module_failure(
                        "failed to scan transitive modules",
                        &module_path,
                        &message,
                        is_explicit,
                        &mut failed_transitives,
                    )?;
                    continue;
                }
            };

            if !listed.package_errors.is_empty() && is_explicit {
                for err in &listed.package_errors {
                    if let Some(hint) = local_child_hint_in_message(workspace, &err.message, locals)
                    {
                        return Err(undeclared_local_error(&format!("`{}`", err.package), &hint));
                    }
                }
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

            report_unresolved_transitives(workspace, &module_path, &listed.unresolved, locals)?;

            graph.expand(module_path, module_version, listed.modules.clone());

            for next in listed.modules {
                queue.push(next);
            }
        }

        let drift = detect_mvs_drift(workspace, &graph);
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

    Ok(graph)
}

/// The `ReplacementTarget` a module's typedef cache is keyed by, if it is a declared replacement.
fn replacement_for<'a>(
    module_path: &str,
    replacements: &'a HashMap<String, deps::ReplacementSource>,
) -> Option<deps::ReplacementTarget<'a>> {
    replacements.get(module_path).map(|source| match source {
        deps::ReplacementSource::Module { path, version } => {
            deps::ReplacementTarget::Module(deps::Replacement { path, version })
        }
        deps::ReplacementSource::Local { .. } => deps::ReplacementTarget::LocalDirectory,
    })
}

/// Declared local modules as `(module path, absolute directory)`.
pub(crate) fn declared_local_dirs(
    replacements: &HashMap<String, deps::ReplacementSource>,
    project_root: &Path,
) -> Vec<(String, PathBuf)> {
    replacements
        .iter()
        .filter_map(|(module, source)| match source {
            deps::ReplacementSource::Local { path } => {
                let declared = Path::new(path);
                let joined = if declared.is_absolute() {
                    declared.to_path_buf()
                } else {
                    project_root.join(declared)
                };
                Some((module.clone(), joined.canonicalize().unwrap_or(joined)))
            }
            deps::ReplacementSource::Module { .. } => None,
        })
        .collect()
}

/// Where a declared local module's own `go.mod` maps an unresolved import.
struct LocalChildHint {
    parent_module: String,
    child_module: String,
    directory: PathBuf,
    /// Nested inside the parent's directory rather than named by a `replace`.
    nested: bool,
}

impl LocalChildHint {
    fn describe(&self) -> String {
        if self.nested {
            format!(
                "`{}` is a separate module nested in local module `{}` at `{}`, but `{}` is not declared in `lisette.toml`",
                self.child_module,
                self.parent_module,
                self.directory.display(),
                self.child_module
            )
        } else {
            format!(
                "local module `{}` resolves `{}` from `{}`, but `{}` is not declared in `lisette.toml`",
                self.parent_module,
                self.child_module,
                self.directory.display(),
                self.child_module
            )
        }
    }
}

/// Powers diagnostics only, never resolution: the child's identity comes from
/// its own `go.mod` when the user declares it.
fn find_local_child(
    workspace: &GoWorkspace,
    locals: &[(String, PathBuf)],
    covers: impl Fn(&str) -> bool,
) -> Option<LocalChildHint> {
    let mut best: Option<LocalChildHint> = None;
    for (parent_module, parent_dir) in locals {
        let Ok(summary) = workspace.read_go_mod_summary(&parent_dir.join("go.mod")) else {
            continue;
        };
        for (old, new) in summary.directory_replaces {
            let declared = locals.iter().any(|(module, _)| module == &old);
            if declared || !covers(&old) {
                continue;
            }
            if best
                .as_ref()
                .is_some_and(|prev| prev.child_module.len() >= old.len())
            {
                continue;
            }
            let new_path = Path::new(&new);
            let joined = if new_path.is_absolute() {
                new_path.to_path_buf()
            } else {
                parent_dir.join(new_path)
            };
            best = Some(LocalChildHint {
                parent_module: parent_module.clone(),
                child_module: old,
                directory: joined.canonicalize().unwrap_or(joined),
                nested: false,
            });
        }
    }
    best
}

fn local_child_hint(
    workspace: &GoWorkspace,
    import: &str,
    locals: &[(String, PathBuf)],
) -> Option<LocalChildHint> {
    find_local_child(workspace, locals, |old| {
        import == old || import.starts_with(&format!("{}/", old))
    })
    .or_else(|| nested_local_child(import, locals))
}

/// A nested module is excluded from its parent on-disk module, so a package
/// under it resolves nowhere until the nested module is declared itself.
fn nested_local_child(pkg_path: &str, locals: &[(String, PathBuf)]) -> Option<LocalChildHint> {
    let (parent_module, parent_dir) = locals
        .iter()
        .filter(|(module, _)| pkg_path.starts_with(&format!("{}/", module)))
        .max_by_key(|(module, _)| module.len())?;
    let remainder = &pkg_path[parent_module.len() + 1..];

    let mut child_module = parent_module.clone();
    let mut directory = parent_dir.clone();
    for segment in remainder.split('/') {
        child_module = format!("{}/{}", child_module, segment);
        directory = directory.join(segment);
        if directory.join("go.mod").exists() {
            return Some(LocalChildHint {
                parent_module: parent_module.clone(),
                child_module,
                directory,
                nested: true,
            });
        }
    }
    None
}

/// Every undeclared module nested inside a declared local module's directory.
fn nested_local_candidates(locals: &[(String, PathBuf)]) -> Vec<LocalChildHint> {
    let declared: HashSet<&str> = locals.iter().map(|(module, _)| module.as_str()).collect();
    let mut candidates = Vec::new();
    for (parent_module, parent_dir) in locals {
        collect_nested_modules(
            parent_dir,
            parent_module,
            parent_module,
            &declared,
            &mut candidates,
        );
    }
    candidates
}

fn collect_nested_modules(
    dir: &Path,
    module: &str,
    parent_module: &str,
    declared: &HashSet<&str>,
    out: &mut Vec<LocalChildHint>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !path.is_dir() || name.starts_with('.') {
            continue;
        }
        let child_module = format!("{}/{}", module, name);
        if path.join("go.mod").exists() {
            if !declared.contains(child_module.as_str()) {
                out.push(LocalChildHint {
                    parent_module: parent_module.to_string(),
                    child_module,
                    directory: path,
                    nested: true,
                });
            }
            continue;
        }
        collect_nested_modules(&path, &child_module, parent_module, declared, out);
    }
}

fn local_child_hint_in_message(
    workspace: &GoWorkspace,
    message: &str,
    locals: &[(String, PathBuf)],
) -> Option<LocalChildHint> {
    find_local_child(workspace, locals, |old| message_names_module(message, old))
}

/// Whether `message` names `module` at path boundaries: `example.com/foo`
/// matches `example.com/foo/sub` but never `example.com/foobar`.
fn message_names_module(message: &str, module: &str) -> bool {
    let extends_segment = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~');
    message.match_indices(module).any(|(start, _)| {
        let before_ok = message[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !extends_segment(c) && c != '/');
        let after_ok = message[start + module.len()..]
            .chars()
            .next()
            .is_none_or(|c| !extends_segment(c));
        before_ok && after_ok
    })
}

fn undeclared_local_error(entity: &str, hint: &LocalChildHint) -> i32 {
    cli_error!(
        "Undeclared local module",
        format!(
            "{} needs `{}`: {}",
            entity,
            hint.child_module,
            hint.describe()
        ),
        format!("Run `lis add --path {}`", hint.directory.display())
    );
    1
}

fn report_unresolved_transitives(
    workspace: &GoWorkspace,
    scanned_module: &str,
    unresolved: &[UnresolvedTransitive],
    locals: &[(String, PathBuf)],
) -> Result<(), i32> {
    for entry in unresolved {
        if let Some(hint) = local_child_hint(workspace, &entry.import, locals) {
            return Err(undeclared_local_error(
                &format!("`{}`", entry.import),
                &hint,
            ));
        }
        print_warning(&format!(
            "could not resolve transitive import `{}` from `{}`: {}; declare it manually with `lis add {}` if your code references it",
            entry.import, scanned_module, entry.message, entry.import
        ));
    }
    Ok(())
}

/// Go ignores a dependency's replace directives, so a local child fails
/// resolution until declared itself. Appends the `lis add --path` remedy.
pub(crate) fn augment_go_error_with_local_hint(
    error: String,
    project_root: &Path,
    target_dir: &Path,
    manifest: &deps::Manifest,
) -> String {
    let replacements = declared_replacements(manifest);
    let locals = declared_local_dirs(&replacements, project_root);
    if locals.is_empty() {
        return error;
    }
    let workspace = GoWorkspace::new(target_dir, target_dir, Target::host());
    let hint = find_local_child(&workspace, &locals, |old| {
        message_names_module(&error, old) && !replacements.contains_key(old)
    })
    .or_else(|| {
        nested_local_candidates(&locals)
            .into_iter()
            .find(|candidate| message_names_module(&error, &candidate.child_module))
    });
    match hint {
        Some(hint) => format!(
            "{}\n · {}. Run `lis add --path {}`",
            error,
            hint.describe(),
            hint.directory.display()
        ),
        None => error,
    }
}

/// The declared replacement redirects, keyed by original module path.
pub(crate) fn declared_replacements(
    manifest: &deps::Manifest,
) -> HashMap<String, deps::ReplacementSource> {
    manifest
        .go_deps()
        .into_iter()
        .filter_map(|(module, dep)| match dep {
            deps::GoDependency::Replaced { source, .. } => Some((module, source)),
            deps::GoDependency::Remote { .. } => None,
        })
        .collect()
}

/// A package waiting to be bindgenned, pinned to its module's version.
struct QueuedPackage {
    module: String,
    version: String,
    package: String,
}

/// The identity the cache walk dedupes on: a package within its module.
#[derive(PartialEq, Eq, Hash)]
struct PackageKey {
    module: String,
    package: String,
}

impl QueuedPackage {
    fn key(&self) -> PackageKey {
        PackageKey {
            module: self.module.clone(),
            package: self.package.clone(),
        }
    }
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
    replacements: &HashMap<String, deps::ReplacementSource>,
) -> Result<Vec<BindgennedPackage>, i32> {
    let mut visited: HashSet<PackageKey> = HashSet::new();
    let mut queue: Vec<QueuedPackage> = Vec::new();
    let mut bindgenned: Vec<BindgennedPackage> = Vec::new();

    let seed_packages = seed_cache_walk(
        &dep.canonical_module,
        &dep.requested_package,
        workspace,
        &mut queue,
    )?;

    while let Some(entry) = queue.pop() {
        if !visited.insert(entry.key()) {
            continue;
        }

        let is_seed = seed_packages.contains(&entry.key());
        let module = GoModule {
            path: &entry.module,
            version: &entry.version,
            replacement: replacement_for(&entry.module, replacements),
        };

        match workspace.reconcile_package(module, &entry.package) {
            Ok(stubs) => {
                warn_stubbed(&stubs);
                bindgenned.push(BindgennedPackage {
                    module: entry.module.clone(),
                    version: entry.version.clone(),
                    package: entry.package.clone(),
                });
            }
            Err(message) => {
                if is_seed {
                    error!("failed to bindgen package", message);
                    return Err(1);
                }
                print_warning(&format!(
                    "skipping transitive {}: {}",
                    entry.package, message
                ));
                continue;
            }
        }

        let imports = match workspace.imports_of(module, &entry.package) {
            Ok(imports) => imports,
            Err(message) => {
                print_warning(&format!(
                    "skipping import-walk for {}: {}",
                    entry.package, message
                ));
                continue;
            }
        };

        for import in imports {
            let Some(next) = classify_import(import, &entry, workspace, module_graph) else {
                continue;
            };
            if !visited.contains(&next.key()) {
                queue.push(next);
            }
        }
    }

    Ok(bindgenned)
}

/// Where one typedef import sends the cache walk next, if anywhere.
fn classify_import(
    import: String,
    current: &QueuedPackage,
    workspace: &GoWorkspace,
    module_graph: &mut GraphResult,
) -> Option<QueuedPackage> {
    if deps::is_stdlib(&import) {
        return None;
    }
    let containing = match workspace.find_containing_module(&import) {
        Ok(info) if !info.path.is_empty() => info,
        _ => {
            print_warning(&format!(
                "could not resolve containing module for `{}` (referenced by {})",
                import, current.package
            ));
            return None;
        }
    };
    if containing.path == current.module {
        return Some(QueuedPackage {
            module: containing.path,
            version: current.version.clone(),
            package: import,
        });
    }

    // Record cache-walk-discovered modules so the manifest declares
    // every module whose typedef ends up in the cache.
    let next_version = resolve_discovered_version(
        &containing.path,
        containing.version,
        &import,
        workspace,
        module_graph,
    )?;

    module_graph.record_dependency(
        current.module.clone(),
        current.version.clone(),
        containing.path.clone(),
    );

    Some(QueuedPackage {
        module: containing.path,
        version: next_version,
        package: import,
    })
}

/// The graph's pin, else the version `go list` reported, else a fresh query.
fn resolve_discovered_version(
    containing_module: &str,
    listed_version: String,
    import: &str,
    workspace: &GoWorkspace,
    module_graph: &mut GraphResult,
) -> Option<String> {
    if let Some(version) = module_graph.version(containing_module) {
        return Some(version.to_string());
    }
    let resolved = if !listed_version.is_empty() {
        listed_version
    } else {
        match workspace.query_version(containing_module) {
            Ok(version) => version,
            Err(message) => {
                print_warning(&format!("skipping transitive {}: {}", import, message));
                return None;
            }
        }
    };
    module_graph.discover(containing_module.to_string(), resolved.clone());
    Some(resolved)
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
    replacements: &HashMap<String, deps::ReplacementSource>,
) {
    for entry in bindgenned {
        let Some(current) = graph.version(&entry.module) else {
            continue;
        };
        if current == entry.version {
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

/// Exhaustively scan modules known only through typedef imports, until the
/// graph is closed under MVS drift. Failures are warnings since these are all
/// transitives.
fn expand_unwalked_modules(
    workspace: &GoWorkspace,
    graph: &mut GraphResult,
    locals: &[(String, PathBuf)],
) -> Result<(), i32> {
    let mut failed: HashSet<String> = HashSet::new();

    let mut queue = graph.discovered_modules();

    loop {
        while let Some(module_path) = queue.pop() {
            if graph.is_expanded(&module_path) {
                continue;
            }

            if graph.version(&module_path).is_none() {
                match workspace.query_version(&module_path) {
                    Ok(version) => graph.discover(module_path.clone(), version),
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

            report_unresolved_transitives(workspace, &module_path, &listed.unresolved, locals)?;

            let Some(version) = graph.version(&module_path).map(str::to_string) else {
                continue;
            };
            graph.expand(module_path, version, listed.modules.clone());

            for next in listed.modules {
                if !graph.is_expanded(&next) {
                    queue.push(next);
                }
            }
        }

        let drift = detect_mvs_drift(workspace, graph);
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
            graph.rediscover(module.clone(), new_version);
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
    queue: &mut Vec<QueuedPackage>,
) -> Result<HashSet<PackageKey>, i32> {
    let version = match workspace.query_version(canonical_module) {
        Ok(version) => version,
        Err(message) => {
            error!("failed to resolve module version", message);
            return Err(1);
        }
    };

    let push_seed = |queue: &mut Vec<_>, seeds: &mut HashSet<_>, package: String| {
        seeds.insert(PackageKey {
            module: canonical_module.to_string(),
            package: package.clone(),
        });
        queue.push(QueuedPackage {
            module: canonical_module.to_string(),
            version: version.clone(),
            package,
        });
    };

    let mut seeds: HashSet<PackageKey> = HashSet::new();

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
fn detect_mvs_drift(workspace: &GoWorkspace, graph: &GraphResult) -> DriftReport {
    let mut report = DriftReport::default();
    let snapshot: Vec<(String, String)> = graph
        .modules
        .iter()
        .map(|(module, state)| (module.clone(), state.version().to_string()))
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

#[derive(Default)]
pub(crate) struct ManifestChanges {
    pub(crate) trimmed: Vec<deps::TrimmedVia>,
    pub(crate) promoted: Vec<String>,
    pub(crate) removed: Vec<String>,
}

impl ManifestChanges {
    pub(crate) fn extend(&mut self, other: Self) {
        self.trimmed.extend(other.trimmed);
        self.promoted.extend(other.promoted);
        self.removed.extend(other.removed);
    }
}

/// Upsert one manifest entry, mapping a write failure to the exit code.
fn write_manifest_dependency(
    project_root: &Path,
    module: &str,
    dependency: &deps::GoDependency,
) -> Result<(), i32> {
    upsert_go_dependency(project_root, module, dependency).map_err(|message| {
        error!("failed to update manifest", message);
        1
    })
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
    workspace: &GoWorkspace,
    graph: &GraphResult,
    root: RootWrite<'_>,
) -> Result<Vec<DirectUpgrade>, i32> {
    let existing_deps = manifest.go_deps();
    let transitives = graph.transitive_map(added_dep);
    let mut upgraded: Vec<DirectUpgrade> = Vec::new();

    let root_dependency = match root {
        RootWrite::Replaced(replaced_root) => {
            let via = match replaced_root.mode {
                ReplacedRootMode::SyncPreserveVia => existing_deps
                    .get(added_dep)
                    .and_then(|dependency| dependency.via().map(<[String]>::to_vec)),
                ReplacedRootMode::AddDirect => None,
            };
            deps::GoDependency::Replaced {
                source: replaced_root.identity.clone(),
                via,
            }
        }
        RootWrite::Remote { fallback_version } => {
            let added_dep_version = graph.version(added_dep).unwrap_or(fallback_version);
            deps::GoDependency::Remote {
                version: added_dep_version.to_string(),
                via: None,
            }
        }
    };
    write_manifest_dependency(project_root, added_dep, &root_dependency)?;

    let mut sorted_transitives: Vec<(&String, &Vec<String>)> = transitives.iter().collect();
    sorted_transitives.sort_by(|a, b| a.0.cmp(b.0));

    for (module_path, parents) in &sorted_transitives {
        let version = match graph.version(module_path) {
            Some(version) => version,
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
                write_manifest_dependency(
                    project_root,
                    module_path,
                    &deps::GoDependency::Remote {
                        version: version.to_string(),
                        via: None,
                    },
                )?;
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
        match existing {
            Some(replaced @ deps::GoDependency::Replaced { .. }) => {
                write_manifest_dependency(project_root, module_path, &replaced.with_via(Some(via)))?
            }
            _ => write_manifest_dependency(
                project_root,
                module_path,
                &deps::GoDependency::Remote {
                    version: version.to_string(),
                    via: Some(via),
                },
            )?,
        };
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
            write_manifest_dependency(project_root, dep_path, &dep.with_via(Some(Vec::new())))?;
            continue;
        }

        match dep {
            deps::GoDependency::Replaced { .. } => {
                write_manifest_dependency(project_root, dep_path, &dep.with_via(Some(filtered)))?;
            }
            deps::GoDependency::Remote { .. } => {
                let dep_version = workspace.query_version(dep_path).map_err(|msg| {
                    error!("failed to resolve module version", msg);
                    1
                })?;
                write_manifest_dependency(
                    project_root,
                    dep_path,
                    &deps::GoDependency::Remote {
                        version: dep_version,
                        via: Some(filtered),
                    },
                )?;
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
) -> Result<ManifestChanges, i32> {
    let mut changes = ManifestChanges::default();

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
        changes.trimmed.extend(trimmed);
        changes.promoted.extend(report.promoted);
        changes.removed.extend(report.removed);

        if !changed {
            break;
        }
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_names_module_requires_path_boundaries() {
        let module = "example.com/foo";
        assert!(message_names_module(
            "no module provides `example.com/foo`",
            module
        ));
        assert!(message_names_module(
            "unrecognized import path \"example.com/foo\"",
            module
        ));
        assert!(message_names_module(
            "missing example.com/foo/sub package",
            module
        ));
        assert!(message_names_module("example.com/foo", module));

        assert!(!message_names_module(
            "no module provides example.com/foobar",
            module
        ));
        assert!(!message_names_module(
            "no module provides example.com/foo.v2",
            module
        ));
        assert!(!message_names_module(
            "path is other.com/example.com/foo",
            module
        ));
        assert!(!message_names_module("unrelated error", module));
    }

    fn nested_fixture() -> (tempfile::TempDir, Vec<(String, PathBuf)>) {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("parent");
        std::fs::create_dir_all(parent.join("child/sub")).unwrap();
        std::fs::create_dir_all(parent.join("plain")).unwrap();
        std::fs::write(parent.join("go.mod"), "module example.com/x/parent\n").unwrap();
        std::fs::write(
            parent.join("child/go.mod"),
            "module example.com/x/parent/child\n",
        )
        .unwrap();
        let locals = vec![("example.com/x/parent".to_string(), parent)];
        (dir, locals)
    }

    #[test]
    fn nested_local_child_stops_at_the_first_nested_go_mod() {
        let (_dir, locals) = nested_fixture();

        let hint = nested_local_child("example.com/x/parent/child/sub", &locals).unwrap();
        assert_eq!(hint.child_module, "example.com/x/parent/child");
        assert!(hint.nested);
        assert!(hint.directory.ends_with("parent/child"));

        assert!(nested_local_child("example.com/x/parent/plain", &locals).is_none());
        assert!(nested_local_child("example.com/other/pkg", &locals).is_none());
    }

    #[test]
    fn nested_local_candidates_skips_declared_modules() {
        let (_dir, mut locals) = nested_fixture();

        let found = nested_local_candidates(&locals);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].child_module, "example.com/x/parent/child");

        let child_dir = locals[0].1.join("child");
        locals.push(("example.com/x/parent/child".to_string(), child_dir));
        assert!(nested_local_candidates(&locals).is_empty());
    }

    #[test]
    fn expanded_leaf_is_not_left_pending() {
        let mut graph = GraphResult::default();
        graph.discover("leaf".to_string(), "v1.0.0".to_string());

        graph.expand("leaf".to_string(), "v1.0.0".to_string(), Vec::new());

        assert!(graph.discovered_modules().is_empty());
    }

    #[test]
    fn expanding_preserves_dependencies_found_by_the_typedef_walk() {
        let mut graph = GraphResult::default();
        graph.record_dependency(
            "parent".to_string(),
            "v1.0.0".to_string(),
            "typedef-child".to_string(),
        );

        graph.expand(
            "parent".to_string(),
            "v1.0.0".to_string(),
            vec!["manifest-child".to_string()],
        );

        assert_eq!(graph.edges()["parent"], ["manifest-child", "typedef-child"]);
    }

    #[test]
    fn version_drift_discards_dependencies_from_the_old_version() {
        let mut graph = GraphResult::default();
        graph.expand(
            "module".to_string(),
            "v1.0.0".to_string(),
            vec!["old-child".to_string()],
        );

        graph.expand("module".to_string(), "v2.0.0".to_string(), Vec::new());

        assert!(graph.edges()["module"].is_empty());
    }
}
