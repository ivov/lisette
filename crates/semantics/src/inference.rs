use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use diagnostics::LocalSink;
use syntax::ParseError;
use syntax::lex::Lexer;
use syntax::parse::{ParseResult, Parser};
use syntax::program::{File, Module};

use deps::TypedefLocator;

use crate::cache::{
    CachedModuleBuild, CompiledModule, ModuleInterface, build_cached_module,
    compute_emit_artifact_hash, compute_module_hash, get_dependency_module_hashes,
    go_stdlib::{self, load_cached_go_module},
    hash_module_source_pair, hash_module_source_pair_refs, is_cache_disabled,
    prelude as prelude_cache, restore_cached_generic_bounds, try_load_cache,
};
use crate::checker::infer::{FileInferenceInput, InferCtx};
use crate::checker::{TaskOutput, TaskState};
use crate::diagnostics::{GoImportSite, emit_for_locator_result};
use crate::facts::Facts;
use crate::loader::{DiscoveredModules, Loader};
use crate::module_graph::{DependencyGraph, ModuleGraphOptions, Roots, build_module_graph};
use crate::prelude::{parse_and_register_prelude, parse_and_register_test_prelude};
use crate::store::{ENTRY_FILE_ID, ENTRY_MODULE_ID, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilePhase {
    #[default]
    Check,
    Emit,
    Test,
}

impl CompilePhase {
    fn includes_tests(self) -> bool {
        matches!(self, Self::Check | Self::Test)
    }

    fn emits(self) -> bool {
        matches!(self, Self::Emit | Self::Test)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectKind {
    #[default]
    Binary,
    Library,
}

/// The filesystem context in which analysis runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisScope {
    Standalone,
    Directory,
    Project(PathBuf),
}

impl AnalysisScope {
    pub(crate) fn is_standalone(&self) -> bool {
        matches!(self, Self::Standalone)
    }

    pub(crate) fn has_project_root(&self) -> bool {
        matches!(self, Self::Project(_))
    }

    fn project_root(&self) -> Option<&Path> {
        match self {
            Self::Project(root) => Some(root),
            Self::Standalone | Self::Directory => None,
        }
    }

    fn into_project_root(self) -> Option<PathBuf> {
        match self {
            Self::Project(root) => Some(root),
            Self::Standalone | Self::Directory => None,
        }
    }
}

pub struct EntryFile {
    source: String,
    filename: String,
    display_path: String,
    parse_mode: EntryParseMode,
}

impl EntryFile {
    /// Creates an entry whose syntax errors stop analysis.
    pub fn new(source: String, filename: String, display_path: String) -> Self {
        Self {
            source,
            filename,
            display_path,
            parse_mode: EntryParseMode::Strict,
        }
    }

    /// Creates an entry whose parser errors retain the valid partial AST.
    /// Lexer errors remain fatal.
    pub fn recovering(source: String, filename: String, display_path: String) -> Self {
        Self {
            source,
            filename,
            display_path,
            parse_mode: EntryParseMode::Recover,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryParseMode {
    Strict,
    Recover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntryParseStatus {
    #[default]
    Clean,
    Recovered,
    Failed,
}

pub struct AnalyzeInput<'a> {
    pub load_siblings: bool,
    pub scope: AnalysisScope,
    pub loader: &'a dyn Loader,
    /// An explicitly supplied file, when the caller has one. Project files not
    /// supplied here are discovered through the loader.
    pub entry: Option<EntryFile>,
    pub compile_phase: CompilePhase,
    pub project_kind: ProjectKind,
    pub locator: &'a TypedefLocator,
    /// Go module path (from `lisette.toml`); folded into the cache emit-artifact
    /// hash so a project rename invalidates Go outputs.
    pub go_module: &'a str,
    /// When true, `analyze` skips both cache load and save. Set by the CLI for
    /// `--sourcemap` Emit so cwd-decorated Go files are not reused across cwds.
    pub disable_cache: bool,
}

pub const PARALLEL_THRESHOLD: usize = 4;

struct CacheCandidate {
    compiled: CompiledModule,
    files: Vec<File>,
    topo_rank: usize,
}

enum PendingModule {
    Entry {
        module_id: String,
        topo_rank: usize,
    },
    Compiled {
        module: CompiledModule,
        topo_rank: usize,
    },
}

impl PendingModule {
    fn module_id(&self) -> &str {
        match self {
            Self::Entry { module_id, .. } => module_id,
            Self::Compiled { module, .. } => &module.module_id,
        }
    }

    fn topo_rank(&self) -> usize {
        match self {
            Self::Entry { topo_rank, .. } | Self::Compiled { topo_rank, .. } => *topo_rank,
        }
    }
}

struct CacheBuildJob {
    module_id: String,
    interface: ModuleInterface,
    file_id_base: u32,
}

struct RegistrationOutput {
    modules: Vec<Arc<Module>>,
    task: TaskOutput,
}

enum LazyGoStdlibCache {
    Disabled,
    Unloaded,
    Missing,
    Loaded(go_stdlib::GoStdlibCache),
}

impl LazyGoStdlibCache {
    fn new(disabled: bool) -> Self {
        if disabled {
            Self::Disabled
        } else {
            Self::Unloaded
        }
    }

    fn get_or_load(&mut self, target: stdlib::Target) -> Option<&go_stdlib::GoStdlibCache> {
        if matches!(self, Self::Unloaded) {
            *self = match go_stdlib::try_load_go_stdlib_cache(target) {
                Some(cache) => Self::Loaded(cache),
                None => Self::Missing,
            };
        }
        match self {
            Self::Loaded(cache) => Some(cache),
            Self::Disabled | Self::Unloaded | Self::Missing => None,
        }
    }

    fn into_module_ids(self) -> Option<HashSet<String>> {
        match self {
            Self::Loaded(cache) => Some(cache.modules.keys().cloned().collect()),
            Self::Disabled | Self::Unloaded | Self::Missing => None,
        }
    }
}

pub struct InferenceOutput {
    pub store: Store,
    pub facts: Facts,
    pub sink: LocalSink,
    pub has_pre_check_errors: bool,
    pub compiled_modules: Vec<CompiledModule>,
    pub cached_modules: HashSet<String>,
    pub cache_root: Option<PathBuf>,
    pub unreachable_modules: Vec<String>,
    pub entry_parse_errors: Vec<ParseError>,
    pub entry_parse_status: EntryParseStatus,
}

struct EntryRegistration {
    filename: Option<String>,
    errors: Vec<ParseError>,
    status: EntryParseStatus,
}

/// Parses and registers the entry file (`main.lis`, or the library root).
fn register_entry_file(
    store: &mut Store,
    sink: &LocalSink,
    entry: Option<EntryFile>,
    include_tests: bool,
) -> EntryRegistration {
    let Some(entry) = entry else {
        return EntryRegistration {
            filename: None,
            errors: Vec::new(),
            status: EntryParseStatus::Clean,
        };
    };

    let (parse_result, status) = parse_entry_file(&entry.source, entry.parse_mode);
    let ParseResult {
        ast,
        errors,
        file_comment,
    } = parse_result;

    if status != EntryParseStatus::Failed {
        if entry.filename.ends_with("_test.lis") {
            sink.push(diagnostics::module_graph::wrong_test_file_suffix(
                &entry.display_path,
            ));
        } else if entry.filename.ends_with(".test.lis") && !include_tests {
            sink.push(diagnostics::module_graph::cannot_emit_test_file(
                &entry.display_path,
            ));
        }
    }
    store.store_entry_file(
        &entry.filename,
        &entry.display_path,
        &entry.source,
        ast,
        file_comment,
    );

    EntryRegistration {
        filename: Some(entry.filename),
        errors,
        status,
    }
}

fn parse_entry_file(source: &str, mode: EntryParseMode) -> (ParseResult, EntryParseStatus) {
    match mode {
        EntryParseMode::Strict => {
            let result = syntax::build_ast(source, ENTRY_FILE_ID);
            let status = if result.failed() {
                EntryParseStatus::Failed
            } else {
                EntryParseStatus::Clean
            };
            (result, status)
        }
        EntryParseMode::Recover => {
            let lex_result = Lexer::new(source, ENTRY_FILE_ID).lex();
            if lex_result.failed() {
                return (
                    ParseResult {
                        ast: Vec::new(),
                        errors: lex_result.errors,
                        file_comment: None,
                    },
                    EntryParseStatus::Failed,
                );
            }

            let result = Parser::new(lex_result.tokens, source).parse();
            let status = if result.failed() {
                EntryParseStatus::Recovered
            } else {
                EntryParseStatus::Clean
            };
            (result, status)
        }
    }
}

/// Loads every other `.lis` file in the entry module's folder as a sibling file.
fn load_sibling_files(
    store: &mut Store,
    sink: &LocalSink,
    loader: &dyn Loader,
    entry_filename: Option<&str>,
    include_tests: bool,
) {
    for (filename, content) in loader.scan_folder(ENTRY_MODULE_ID) {
        if Some(filename.as_str()) == entry_filename {
            continue;
        }
        if filename.ends_with("_test.lis") {
            sink.push(diagnostics::module_graph::wrong_test_file_suffix(
                &content.display_path,
            ));
            continue;
        }
        if !filename.ends_with(".lis")
            || filename.ends_with(".d.lis")
            || (filename.ends_with(".test.lis") && !include_tests)
        {
            continue;
        }
        let file_id = store.new_file_id();
        let result = syntax::build_ast(&content.source, file_id);
        sink.extend_parse_errors(result.errors);
        store.store_file(File {
            id: file_id,
            module_id: ENTRY_MODULE_ID.to_string(),
            name: filename,
            display_path: content.display_path,
            source_path: None,
            source: content.source,
            items: result.ast,
            file_comment: result.file_comment,
        });
    }
}

fn compute_roots(
    project_kind: ProjectKind,
    compile_phase: CompilePhase,
    discovered: &DiscoveredModules,
    entry_module: String,
) -> Roots {
    let include_test_roots = compile_phase.includes_tests();
    match project_kind {
        ProjectKind::Binary => {
            let mut additional = match compile_phase {
                CompilePhase::Check => discovered.production_modules.clone(),
                CompilePhase::Emit | CompilePhase::Test => Vec::new(),
            };
            if include_test_roots {
                additional.extend(discovered.internal_test_roots.iter().cloned());
                additional.extend(discovered.external_test_roots.iter().cloned());
            }
            Roots {
                primary: vec![entry_module],
                additional,
            }
        }
        ProjectKind::Library => {
            let mut additional = Vec::new();
            if include_test_roots {
                additional.extend(discovered.internal_test_roots.iter().cloned());
                additional.extend(discovered.external_test_roots.iter().cloned());
            }
            additional.push(entry_module);
            Roots {
                primary: discovered.production_modules.clone(),
                additional,
            }
        }
    }
}

/// Production modules the primary roots never reached.
fn find_unreachable_modules(
    discovered: &DiscoveredModules,
    graph_result: &crate::module_graph::ModuleGraphResult,
) -> Vec<String> {
    let mut unreachable: Vec<String> = discovered
        .production_modules
        .iter()
        .filter(|m| !graph_result.primary_reachable.contains(m.as_str()))
        .cloned()
        .collect();
    unreachable.sort();
    unreachable
}

#[derive(Clone, Copy)]
enum PreludeCacheStatus {
    Disabled,
    Hit,
    Miss,
}

/// Loads the prelude from cache when possible, else parses and registers it fresh.
fn load_prelude(store: &mut Store, sink: &LocalSink, cache_disabled: bool) -> PreludeCacheStatus {
    if cache_disabled {
        parse_and_register_prelude(store, sink);
        return PreludeCacheStatus::Disabled;
    }

    let hit = prelude_cache::try_load_prelude_cache().is_some_and(|cached| {
        prelude_cache::register_cached_prelude(store, cached);
        true
    });
    if hit {
        PreludeCacheStatus::Hit
    } else {
        parse_and_register_prelude(store, sink);
        PreludeCacheStatus::Miss
    }
}

struct ModuleInferenceInput<'a> {
    graph_result: crate::module_graph::ModuleGraphResult,
    sink: LocalSink,
    module_cache_root: Option<&'a Path>,
    compile_phase: CompilePhase,
    go_module: &'a str,
    cache_disabled: bool,
    prelude_cache: PreludeCacheStatus,
    locator: &'a TypedefLocator,
    scope: &'a AnalysisScope,
}

struct ModuleInferenceOutput {
    facts: Facts,
    cached_modules: HashSet<String>,
    compiled_modules: Vec<CompiledModule>,
    sink: LocalSink,
}

/// Classifies every topo-ordered module as a `go:` import, a cache candidate,
/// or pending registration, then registers and infers whatever was not
/// served from cache.
fn infer_all_modules(store: &mut Store, mut input: ModuleInferenceInput) -> ModuleInferenceOutput {
    let mut checker = TaskState::with_sink(input.sink);

    let mut module_hashes: HashMap<String, u64> = HashMap::default();
    let mut cached_modules: HashSet<String> = HashSet::default();
    let order = std::mem::take(&mut input.graph_result.order);
    let dependencies = &input.graph_result.dependencies;

    let mut go_cache = LazyGoStdlibCache::new(input.cache_disabled);

    let mut to_infer: Vec<PendingModule> = Vec::new();
    let mut candidates: Vec<CacheCandidate> = Vec::new();

    let mut source_hashes: HashMap<String, (u64, u64)> =
        if input.graph_result.files.len() < PARALLEL_THRESHOLD {
            input
                .graph_result
                .files
                .iter()
                .map(|(id, files)| (id.clone(), hash_module_source_pair(files)))
                .collect()
        } else {
            input
                .graph_result
                .files
                .par_iter()
                .map(|(id, files)| (id.clone(), hash_module_source_pair(files)))
                .collect()
        };

    let entry_files: Vec<&File> = store
        .get_module(ENTRY_MODULE_ID)
        .map(|module| module.files.values().collect())
        .unwrap_or_default();
    if !entry_files.is_empty() {
        source_hashes.insert(
            ENTRY_MODULE_ID.to_string(),
            hash_module_source_pair_refs(&entry_files),
        );
    }

    for (topo_rank, module_id) in order.into_iter().enumerate() {
        if module_id.starts_with("go:") {
            if dependencies.is_link_only_module(&module_id) {
                continue;
            }
            register_go_module(
                &mut checker,
                store,
                &module_id,
                input.locator,
                input.scope.is_standalone(),
                &mut go_cache,
            );
            continue;
        }

        let mut files = input
            .graph_result
            .files
            .remove(&module_id)
            .unwrap_or_default();
        if input.scope.has_project_root()
            && store.project_kind == ProjectKind::Library
            && crate::loader::is_external_test_module(&module_id)
        {
            for file in &mut files {
                file.rewrite_import(crate::loader::ROOT_IMPORT, ENTRY_MODULE_ID);
            }
        }
        // Production-only hash drives dependents/emit; all-files hash drives own validity.
        let (production_hash, full_hash) = source_hashes
            .get(&module_id)
            .copied()
            .unwrap_or_else(|| hash_module_source_pair(&files));

        let dep_hashes =
            get_dependency_module_hashes(dependencies.dependencies(&module_id), &module_hashes);
        let production_dep_hashes = get_dependency_module_hashes(
            dependencies.production_dependencies(&module_id),
            &module_hashes,
        );
        let module_hash = compute_module_hash(production_hash, &production_dep_hashes);
        module_hashes.insert(module_id.clone(), module_hash);

        let is_entry = module_id == ENTRY_MODULE_ID;

        let compiled = (!is_entry).then(|| CompiledModule {
            module_id: module_id.clone(),
            artifact_hash: compute_emit_artifact_hash(production_hash, input.go_module),
            full_hash,
            dep_hashes,
        });

        match (input.module_cache_root, compiled) {
            (Some(_), Some(compiled)) => candidates.push(CacheCandidate {
                compiled,
                files,
                topo_rank,
            }),
            (None, compiled) | (Some(_), compiled @ None) => {
                store.store_module(&module_id, files);
                let pending = match compiled {
                    Some(module) => PendingModule::Compiled { module, topo_rank },
                    None => PendingModule::Entry {
                        module_id,
                        topo_rank,
                    },
                };
                to_infer.push(pending);
            }
        }
    }

    let go_cache_module_ids = go_cache.into_module_ids();

    let cache_load = match input.module_cache_root {
        Some(root) => {
            load_cache_candidates(&mut checker, store, candidates, root, input.compile_phase)
        }
        None => {
            debug_assert!(candidates.is_empty());
            CacheLoad::default()
        }
    };
    cached_modules.extend(cache_load.cached);
    to_infer.extend(cache_load.to_infer);

    for pending in &to_infer {
        checker.predeclare_module_types(store, pending.module_id());
    }
    restore_cached_generic_bounds(store, &checker.sink, &cached_modules);

    to_infer.sort_by_key(PendingModule::topo_rank);
    let mut compiled_modules = Vec::new();
    let to_infer: Vec<String> = to_infer
        .into_iter()
        .map(|pending| match pending {
            PendingModule::Entry { module_id, .. } => module_id,
            PendingModule::Compiled { module, .. } => {
                let module_id = module.module_id.clone();
                compiled_modules.push(module);
                module_id
            }
        })
        .collect();

    register_modules(&mut checker, store, &to_infer, dependencies);
    infer_modules(&mut checker, store, &to_infer);

    if !input.cache_disabled {
        let all_go_modules: Vec<String> = store
            .modules
            .keys()
            .filter(|id| id.strip_prefix("go:").is_some_and(deps::is_stdlib))
            .cloned()
            .collect();
        // A non-empty list implies the lazy cache load was attempted.
        let needs_save = !all_go_modules.is_empty()
            && go_cache_module_ids.as_ref().is_none_or(|ids| {
                all_go_modules.len() != ids.len()
                    || all_go_modules.iter().any(|id| !ids.contains(id))
            });
        if needs_save {
            go_stdlib::save_go_stdlib_cache(store, &all_go_modules, input.locator.target());
        }
    }

    if matches!(input.prelude_cache, PreludeCacheStatus::Miss) {
        prelude_cache::save_prelude_cache(store);
    }

    ModuleInferenceOutput {
        facts: checker.facts,
        cached_modules,
        compiled_modules,
        sink: checker.sink,
    }
}

/// Loads, registers, and infers every module, returning the artifacts the
/// post-inference passes consume. Internal, unstable API.
pub fn run_inference(input: AnalyzeInput) -> InferenceOutput {
    let mut store = Store::new();
    store.project_kind = input.project_kind;

    let sink = LocalSink::new();
    let include_tests = input.compile_phase.includes_tests();

    store.init_entry_module();
    let entry = register_entry_file(&mut store, &sink, input.entry, include_tests);
    if entry.status == EntryParseStatus::Failed {
        let checker = TaskState::with_sink(sink);
        return InferenceOutput {
            store,
            facts: checker.facts,
            sink: checker.sink,
            has_pre_check_errors: true,
            compiled_modules: Vec::new(),
            cached_modules: HashSet::default(),
            cache_root: None,
            unreachable_modules: Vec::new(),
            entry_parse_errors: entry.errors,
            entry_parse_status: entry.status,
        };
    }
    if input.load_siblings {
        load_sibling_files(
            &mut store,
            &sink,
            input.loader,
            entry.filename.as_deref(),
            include_tests,
        );
    }

    let entry_module = store.entry_module_id().to_string();
    let discovered = if input.scope.has_project_root() {
        input.loader.discover_modules()
    } else {
        DiscoveredModules::default()
    };

    let roots = compute_roots(
        input.project_kind,
        input.compile_phase,
        &discovered,
        entry_module,
    );

    let graph_result = build_module_graph(
        &mut store,
        roots,
        ModuleGraphOptions {
            loader: Some(input.loader),
            sink: &sink,
            scope: &input.scope,
            locator: input.locator,
            include_tests,
        },
    );

    for cycle in &graph_result.cycles {
        sink.push(diagnostics::module_graph::import_cycle(cycle));
    }
    let unreachable_modules = find_unreachable_modules(&discovered, &graph_result);

    let has_pre_check_errors = sink.has_errors();

    let cache_disabled = is_cache_disabled();
    let prelude_cache = load_prelude(&mut store, &sink, cache_disabled);
    parse_and_register_test_prelude(&mut store, &sink);

    let cache_enabled = !cache_disabled && !input.disable_cache;
    let module_cache_root = if cache_enabled {
        input.scope.project_root()
    } else {
        None
    };
    let module_output = infer_all_modules(
        &mut store,
        ModuleInferenceInput {
            graph_result,
            sink,
            module_cache_root,
            compile_phase: input.compile_phase,
            go_module: input.go_module,
            cache_disabled,
            prelude_cache,
            locator: input.locator,
            scope: &input.scope,
        },
    );
    let cache_root = if cache_enabled {
        input.scope.into_project_root()
    } else {
        None
    };

    InferenceOutput {
        store,
        facts: module_output.facts,
        sink: module_output.sink,
        has_pre_check_errors,
        compiled_modules: module_output.compiled_modules,
        cached_modules: module_output.cached_modules,
        cache_root,
        unreachable_modules,
        entry_parse_errors: entry.errors,
        entry_parse_status: entry.status,
    }
}

/// Registers one `go:` module, reusing the stdlib cache when it covers the package.
fn register_go_module(
    checker: &mut TaskState,
    store: &mut Store,
    module_id: &str,
    locator: &TypedefLocator,
    standalone_mode: bool,
    go_cache: &mut LazyGoStdlibCache,
) {
    let go_pkg = module_id.strip_prefix("go:").unwrap_or(module_id);
    if deps::is_stdlib(go_pkg)
        && let Some(cache) = go_cache.get_or_load(locator.target())
    {
        load_cached_go_module(store, module_id, cache, locator.target());
        if store.has(module_id) {
            return;
        }
    }

    match locator.find_typedef_content(go_pkg) {
        deps::TypedefLocatorResult::Found { content, origin } => {
            checker.parse_and_register_go_module(
                store,
                module_id,
                content.as_ref(),
                origin.into_cache_path(),
                locator,
            );
        }
        other => {
            emit_for_locator_result(
                &other,
                &GoImportSite {
                    import_name: module_id,
                    go_pkg,
                    name_span: None,
                    target: locator.target(),
                    standalone_mode,
                    replace_importer: None,
                },
                &checker.sink,
            );
        }
    }
}

#[derive(Default)]
struct CacheLoad {
    cached: HashSet<String>,
    to_infer: Vec<PendingModule>,
}

/// Merges cache hits into the store and returns the misses to register.
fn load_cache_candidates(
    checker: &mut TaskState,
    store: &mut Store,
    candidates: Vec<CacheCandidate>,
    project_root: &Path,
    compile_phase: CompilePhase,
) -> CacheLoad {
    let load = |c: &CacheCandidate| {
        let expected_artifact_hash = compile_phase.emits().then_some(c.compiled.artifact_hash);
        try_load_cache(
            &c.compiled.module_id,
            c.compiled.full_hash,
            &c.compiled.dep_hashes,
            expected_artifact_hash,
            project_root,
        )
    };
    let loaded: Vec<Option<ModuleInterface>> = if candidates.len() < PARALLEL_THRESHOLD {
        candidates.iter().map(load).collect()
    } else {
        candidates.par_iter().map(load).collect()
    };

    let mut result = CacheLoad::default();
    let mut build_jobs: Vec<CacheBuildJob> = Vec::new();
    for (candidate, interface) in candidates.into_iter().zip(loaded) {
        let Some(interface) = interface else {
            let module_id = candidate.compiled.module_id.clone();
            store.store_module(&module_id, candidate.files);
            result.to_infer.push(PendingModule::Compiled {
                module: candidate.compiled,
                topo_rank: candidate.topo_rank,
            });
            continue;
        };
        let file_id_base = store.reserve_file_ids(interface.files.len() as u32);
        build_jobs.push(CacheBuildJob {
            module_id: candidate.compiled.module_id,
            interface,
            file_id_base,
        });
    }

    let src_base = crate::path::DisplayPathBase::new(&project_root.join("src"));
    let root_base = crate::path::DisplayPathBase::new(project_root);
    let build = |job: CacheBuildJob| {
        build_cached_module(
            job.module_id,
            job.file_id_base,
            job.interface,
            &src_base,
            &root_base,
        )
    };
    let built: Vec<CachedModuleBuild> = if build_jobs.len() < PARALLEL_THRESHOLD {
        build_jobs.into_iter().map(build).collect()
    } else {
        build_jobs.into_par_iter().map(build).collect()
    };

    for build in built {
        let module_id = build.module.id.clone();
        store.insert_prebuilt_module(build.module);
        checker.collect_cached_module_tests(store, &module_id);
        result.cached.insert(module_id);
    }

    result
}

fn register_modules(
    checker: &mut TaskState,
    store: &mut Store,
    to_infer: &[String],
    dependencies: &DependencyGraph,
) {
    if to_infer.len() < PARALLEL_THRESHOLD {
        for module_id in to_infer {
            checker.register_predeclared_module(store, module_id);
        }
        return;
    }

    // Same-wave modules never read each other, so each worker mutates only its
    // own detached module and reads the rest through a snapshot.
    for wave in registration_waves(to_infer, dependencies) {
        if wave.len() == 1 {
            checker.register_predeclared_module(store, &wave[0]);
            continue;
        }

        let detached: Vec<Arc<Module>> = wave
            .into_iter()
            .map(|module_id| {
                store
                    .modules
                    .remove(&module_id)
                    .expect("fresh module must be stored before registration")
            })
            .collect();

        let chunk_size = detached.len().div_ceil(rayon::current_num_threads()).max(1);
        let store_ref: &Store = store;
        let seed = checker.worker_seed();

        let outputs: Vec<RegistrationOutput> = detached
            .into_par_iter()
            .chunks(chunk_size)
            .map(|chunk| {
                let mut worker = seed.spawn();
                let mut view = store_ref.registration_view();
                let mut registered = Vec::with_capacity(chunk.len());
                for module in chunk {
                    let module_id = module.id.clone();
                    view.modules.insert(module_id.clone(), module);
                    worker.register_predeclared_module(&mut view, &module_id);
                    let module = view
                        .modules
                        .remove(&module_id)
                        .expect("registered module must remain in view");
                    registered.push(module);
                }
                RegistrationOutput {
                    modules: registered,
                    task: worker.into_output(),
                }
            })
            .collect();

        let mut task_outputs = Vec::with_capacity(outputs.len());
        for output in outputs {
            for module in output.modules {
                store.modules.insert(module.id.clone(), module);
            }
            task_outputs.push(output.task);
        }
        checker.absorb_outputs(task_outputs);
    }
}

fn infer_modules(checker: &mut TaskState, store: &mut Store, to_infer: &[String]) {
    checker.finalize_equality(store);
    checker.check_pending_generic_bounds(store);
    checker.finalize_tests(store);

    let module_files: Vec<(String, Vec<FileInferenceInput>)> = to_infer
        .iter()
        .map(|module_id| {
            let files = TaskState::take_module_inference_input(store, module_id);
            (module_id.clone(), files)
        })
        .collect();

    if module_files.len() < PARALLEL_THRESHOLD {
        for (module_id, files) in module_files {
            InferCtx::new(checker, store).infer_module(&module_id, files);
        }
    } else {
        let seed = checker.worker_seed();
        let store_ref: &Store = store;

        let outputs: Vec<TaskOutput> = module_files
            .into_par_iter()
            .map(|(module_id, files)| {
                let mut worker = seed.spawn();
                InferCtx::new(&mut worker, store_ref).infer_module(&module_id, files);
                worker.into_output()
            })
            .collect();

        checker.absorb_outputs(outputs);
    }

    checker.install_inferred_files(store);

    checker.check_post_inference_bounds(store);
}

/// Groups topologically ordered modules into dependency waves, so a wave only
/// reads modules registered in earlier waves.
fn registration_waves(modules: &[String], dependencies: &DependencyGraph) -> Vec<Vec<String>> {
    let mut wave_of: HashMap<&str, usize> = HashMap::default();
    let mut waves: Vec<Vec<String>> = Vec::new();
    for module_id in modules {
        let wave = dependencies
            .dependencies(module_id)
            .filter_map(|dep| wave_of.get(dep.as_str()))
            .map(|dep_wave| dep_wave + 1)
            .max()
            .unwrap_or(0);
        wave_of.insert(module_id, wave);
        if waves.len() == wave {
            waves.push(Vec::new());
        }
        waves[wave].push(module_id.clone());
    }
    waves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_entry_parsing_rejects_partial_ast() {
        let (result, status) =
            parse_entry_file("fn valid() {}\nfn incomplete(", EntryParseMode::Strict);

        assert_eq!(
            (status, result.ast.is_empty()),
            (EntryParseStatus::Failed, true)
        );
    }

    #[test]
    fn recovering_entry_parsing_keeps_partial_ast() {
        let (result, status) =
            parse_entry_file("fn valid() {}\nfn incomplete(", EntryParseMode::Recover);

        assert_eq!(
            (status, result.ast.is_empty()),
            (EntryParseStatus::Recovered, false)
        );
    }

    #[test]
    fn recovering_entry_parsing_still_rejects_lex_errors() {
        let (result, status) =
            parse_entry_file("fn main() { \"unterminated }", EntryParseMode::Recover);

        assert_eq!(
            (
                status,
                result
                    .errors
                    .first()
                    .is_some_and(|error| error.code.starts_with("lex.")),
            ),
            (EntryParseStatus::Failed, true)
        );
    }
}
