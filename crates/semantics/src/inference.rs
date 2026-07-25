use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::program::{File, Module};

use deps::TypedefLocator;

use crate::cache::{
    CachedModuleBuild, CompiledModule, ModuleInterface, build_cached_module,
    compute_emit_artifact_hash, compute_module_hash, get_dependency_module_hashes,
    go_stdlib::{self, load_cached_go_module},
    hash_module_source_pair, is_cache_disabled, prelude as prelude_cache,
    restore_cached_generic_bounds, try_load_cache,
};
use crate::checker::infer::InferCtx;
use crate::checker::{TaskOutput, TaskState};
use crate::diagnostics::{GoImportSite, emit_for_locator_result};
use crate::facts::Facts;
use crate::loader::{DiscoveredModules, Loader};
use crate::module_graph::{ModuleGraphOptions, Roots, build_module_graph};
use crate::prelude::{parse_and_register_prelude, parse_and_register_test_prelude};
use crate::store::{ENTRY_MODULE_ID, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilePhase {
    #[default]
    Check,
    Emit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectKind {
    #[default]
    Binary,
    Library,
}

#[derive(Debug, Clone)]
pub struct SemanticConfig {
    pub run_lints: bool,
    pub standalone_mode: bool,
    pub load_siblings: bool,
}

pub struct EntryFile {
    pub source: String,
    pub filename: String,
    pub display_path: String,
    pub ast: Vec<Expression>,
    pub file_comment: Option<String>,
}

pub struct AnalyzeInput<'a> {
    pub config: SemanticConfig,
    pub loader: &'a dyn Loader,
    /// `None` for a library, whose root files load as siblings.
    pub entry: Option<EntryFile>,
    pub project_root: Option<PathBuf>,
    pub compile_phase: CompilePhase,
    pub project_kind: ProjectKind,
    pub emit_tests: bool,
    pub locator: TypedefLocator,
    /// Go module path (from `lisette.toml`); folded into the cache emit-artifact
    /// hash so a project rename invalidates Go outputs.
    pub go_module: String,
    /// When true, `analyze` skips both cache load and save. Set by the CLI for
    /// `--sourcemap` Emit so cwd-decorated Go files are not reused across cwds.
    pub disable_cache: bool,
}

pub const PARALLEL_THRESHOLD: usize = 4;

struct CacheCandidate {
    compiled: CompiledModule,
    files: Vec<File>,
    topo_rank: usize,
    expected_artifact_hash: Option<u64>,
}

struct PendingModule {
    module_id: String,
    topo_rank: usize,
}

struct CacheBuildJob {
    module_id: String,
    interface: ModuleInterface,
    file_id_base: u32,
}

struct RegistrationOutput {
    modules: Vec<(String, Arc<Module>)>,
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
    pub ufcs_methods: HashSet<(String, String)>,
    pub sink: LocalSink,
    pub has_pre_check_errors: bool,
    pub compiled_modules: Vec<CompiledModule>,
    pub cached_modules: HashSet<String>,
    pub cache_enabled: bool,
    pub unreachable_modules: Vec<String>,
}

/// Loads, registers, and infers every module, returning the artifacts the
/// post-inference passes consume. Internal, unstable API.
pub fn run_inference(input: AnalyzeInput) -> InferenceOutput {
    let mut store = Store::new();
    store.project_kind = input.project_kind;

    let sink = LocalSink::new();

    let include_tests = input.compile_phase == CompilePhase::Check || input.emit_tests;

    store.init_entry_module();
    let entry_filename = input.entry.map(|entry| {
        if entry.filename.ends_with("_test.lis") {
            sink.push(diagnostics::module_graph::wrong_test_file_suffix(
                &entry.display_path,
            ));
        } else if entry.filename.ends_with(".test.lis") && !include_tests {
            sink.push(diagnostics::module_graph::cannot_emit_test_file(
                &entry.display_path,
            ));
        }
        store.store_entry_file(
            &entry.filename,
            &entry.display_path,
            &entry.source,
            entry.ast,
            entry.file_comment,
        );
        entry.filename
    });

    if input.config.load_siblings {
        for (filename, content) in input.loader.scan_folder(ENTRY_MODULE_ID) {
            if Some(&filename) == entry_filename.as_ref() {
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
            store.store_file(File::new(
                ENTRY_MODULE_ID,
                &filename,
                &content.display_path,
                &content.source,
                result.ast,
                result.file_comment,
                file_id,
            ));
        }
    }

    let entry_module = store.entry_module_id().to_string();
    let discovered = if input.project_root.is_some() {
        input.loader.discover_modules()
    } else {
        DiscoveredModules::default()
    };

    let include_test_roots = input.compile_phase == CompilePhase::Check || input.emit_tests;

    let roots = match input.project_kind {
        ProjectKind::Binary => {
            let mut additional = match input.compile_phase {
                CompilePhase::Check => discovered.production_modules.clone(),
                _ => Vec::new(),
            };
            if include_test_roots {
                additional.extend(discovered.test_roots.iter().cloned());
            }
            Roots {
                primary: vec![entry_module],
                additional,
            }
        }
        ProjectKind::Library => {
            let mut additional = if include_test_roots {
                discovered.test_roots.clone()
            } else {
                Vec::new()
            };
            additional.push(entry_module);
            Roots {
                primary: discovered.production_modules.clone(),
                additional,
            }
        }
    };

    let mut graph_result = build_module_graph(
        &mut store,
        roots,
        ModuleGraphOptions {
            loader: Some(input.loader),
            sink: &sink,
            standalone_mode: input.config.standalone_mode,
            locator: &input.locator,
            include_tests,
        },
    );

    for cycle in &graph_result.cycles {
        sink.push(diagnostics::module_graph::import_cycle(cycle));
    }

    let mut unreachable_modules: Vec<String> = discovered
        .production_modules
        .iter()
        .filter(|m| !graph_result.primary_reachable.contains(m.as_str()))
        .cloned()
        .collect();
    unreachable_modules.sort();

    let has_pre_check_errors = sink.has_errors();

    let cache_disabled = is_cache_disabled();

    let prelude_cache_hit = if cache_disabled {
        false
    } else if let Some(cached) = prelude_cache::try_load_prelude_cache() {
        prelude_cache::register_cached_prelude(&mut store, cached);
        true
    } else {
        false
    };

    if !prelude_cache_hit {
        parse_and_register_prelude(&mut store, &sink);
    }
    parse_and_register_test_prelude(&mut store, &sink);

    let cache_enabled = input.project_root.is_some() && !cache_disabled && !input.disable_cache;
    let check_go_files = input.compile_phase == CompilePhase::Emit;

    let (facts, cached_modules, compiled_modules, ufcs_methods, sink) = {
        let mut checker = TaskState::with_sink(sink);
        checker.extend_ufcs_methods(crate::prelude::compute_prelude_ufcs(&store));

        let mut module_hashes: HashMap<String, u64> = HashMap::default();
        let mut cached_modules: HashSet<String> = HashSet::default();
        let mut compiled_modules: Vec<CompiledModule> = vec![];

        let order = std::mem::take(&mut graph_result.order);
        let edges = &graph_result.edges;
        let production_edges = &graph_result.production_edges;

        let mut go_cache = LazyGoStdlibCache::new(cache_disabled);

        let mut to_infer: Vec<PendingModule> = Vec::new();
        let mut candidates: Vec<CacheCandidate> = Vec::new();

        let source_hashes: HashMap<String, (u64, u64)> =
            if graph_result.files.len() < PARALLEL_THRESHOLD {
                graph_result
                    .files
                    .iter()
                    .map(|(id, files)| (id.clone(), hash_module_source_pair(files)))
                    .collect()
            } else {
                graph_result
                    .files
                    .par_iter()
                    .map(|(id, files)| (id.clone(), hash_module_source_pair(files)))
                    .collect()
            };

        for (topo_rank, module_id) in order.into_iter().enumerate() {
            if module_id.starts_with("go:") {
                if graph_result.link_only_modules.contains(&module_id) {
                    continue;
                }
                register_go_module(
                    &mut checker,
                    &mut store,
                    &module_id,
                    &input.locator,
                    input.config.standalone_mode,
                    &mut go_cache,
                );
                continue;
            }

            if store.is_visited(&module_id) {
                continue;
            }

            let files = graph_result.files.remove(&module_id).unwrap_or_default();
            // Production-only hash drives dependents/emit; all-files hash drives own validity.
            let (production_hash, full_hash) = source_hashes
                .get(&module_id)
                .copied()
                .unwrap_or_else(|| hash_module_source_pair(&files));

            let dep_hashes = get_dependency_module_hashes(&module_id, edges, &module_hashes);
            let production_dep_hashes =
                get_dependency_module_hashes(&module_id, production_edges, &module_hashes);
            let module_hash = compute_module_hash(production_hash, &production_dep_hashes);
            module_hashes.insert(module_id.clone(), module_hash);

            let is_entry = module_id == ENTRY_MODULE_ID;

            let compiled = (!is_entry).then(|| CompiledModule {
                module_id: module_id.clone(),
                module_hash,
                production_hash,
                full_hash,
                dep_hashes,
            });

            let expected_artifact_hash = check_go_files
                .then(|| compute_emit_artifact_hash(production_hash, &input.go_module));

            match (cache_enabled, compiled) {
                (true, Some(compiled)) => candidates.push(CacheCandidate {
                    compiled,
                    files,
                    topo_rank,
                    expected_artifact_hash,
                }),
                (_, compiled) => {
                    store.store_module(&module_id, files);
                    if let Some(compiled) = compiled {
                        compiled_modules.push(compiled);
                    }
                    to_infer.push(PendingModule {
                        module_id,
                        topo_rank,
                    });
                }
            }
        }

        let go_cache_module_ids = go_cache.into_module_ids();

        let cache_load = load_cache_candidates(
            &mut checker,
            &mut store,
            candidates,
            input.project_root.as_deref(),
            check_go_files,
        );
        compiled_modules.extend(cache_load.compiled);
        cached_modules.extend(cache_load.cached);
        to_infer.extend(cache_load.to_infer);

        for pending in &to_infer {
            checker.predeclare_module_types(&mut store, &pending.module_id);
        }
        restore_cached_generic_bounds(&mut store, &checker.sink, &cached_modules);

        to_infer.sort_by_key(|pending| pending.topo_rank);
        let to_infer: Vec<String> = to_infer
            .into_iter()
            .map(|pending| pending.module_id)
            .collect();

        let test_ids: Vec<u32> = to_infer
            .iter()
            .filter_map(|module_id| store.get_module(module_id))
            .flat_map(|module| {
                module
                    .files
                    .values()
                    .filter(|file| file.is_test())
                    .map(|file| file.id)
            })
            .collect();
        store.test_file_ids.extend(test_ids);

        register_modules(&mut checker, &mut store, &to_infer, edges);
        infer_modules(&mut checker, &mut store, &to_infer);

        if !cache_disabled {
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
                go_stdlib::save_go_stdlib_cache(&store, &all_go_modules, input.locator.target());
            }
        }

        if !cache_disabled && !prelude_cache_hit {
            prelude_cache::save_prelude_cache(&store);
        }

        let ufcs_methods = checker.take_ufcs_methods();

        (
            checker.facts,
            cached_modules,
            compiled_modules,
            ufcs_methods,
            checker.sink,
        )
    };

    InferenceOutput {
        store,
        facts,
        ufcs_methods,
        sink,
        has_pre_check_errors,
        compiled_modules,
        cached_modules,
        cache_enabled,
        unreachable_modules,
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
        if store.is_visited(module_id) {
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
    compiled: Vec<CompiledModule>,
    cached: HashSet<String>,
    to_infer: Vec<PendingModule>,
}

/// Merges cache hits into the store and returns the misses to register.
fn load_cache_candidates(
    checker: &mut TaskState,
    store: &mut Store,
    candidates: Vec<CacheCandidate>,
    project_root: Option<&Path>,
    check_go_files: bool,
) -> CacheLoad {
    let load = |c: &CacheCandidate| {
        project_root.and_then(|root| {
            try_load_cache(
                &c.compiled.module_id,
                c.compiled.full_hash,
                &c.compiled.dep_hashes,
                c.expected_artifact_hash,
                root,
                check_go_files,
            )
        })
    };
    let loaded: Vec<Option<ModuleInterface>> = if candidates.len() < PARALLEL_THRESHOLD {
        candidates.iter().map(load).collect()
    } else {
        candidates.par_iter().map(load).collect()
    };

    let mut result = CacheLoad::default();
    let mut build_jobs: Vec<CacheBuildJob> = Vec::new();
    let mut discarded: Vec<Vec<File>> = Vec::new();
    for (candidate, interface) in candidates.into_iter().zip(loaded) {
        let Some(interface) = interface else {
            let module_id = candidate.compiled.module_id.clone();
            store.store_module(&module_id, candidate.files);
            result.compiled.push(candidate.compiled);
            result.to_infer.push(PendingModule {
                module_id,
                topo_rank: candidate.topo_rank,
            });
            continue;
        };
        let file_id_base = store.reserve_file_ids(interface.files.len() as u32);
        if !candidate.files.is_empty() {
            discarded.push(candidate.files);
        }
        build_jobs.push(CacheBuildJob {
            module_id: candidate.compiled.module_id,
            interface,
            file_id_base,
        });
    }

    let Some(root) = project_root else {
        return result;
    };
    let display_base = crate::path::DisplayPathBase::new(&root.join("src"));
    let build = |job: CacheBuildJob| {
        build_cached_module(
            job.module_id,
            job.file_id_base,
            job.interface,
            &display_base,
        )
    };
    let run_build = || -> Vec<CachedModuleBuild> {
        if build_jobs.len() < PARALLEL_THRESHOLD {
            build_jobs.into_iter().map(build).collect()
        } else {
            build_jobs.into_par_iter().map(build).collect()
        }
    };
    let built: Vec<CachedModuleBuild> = if discarded.is_empty() {
        run_build()
    } else {
        rayon::join(run_build, move || discarded.into_par_iter().for_each(drop)).0
    };

    for build in built {
        checker.extend_ufcs_methods(build.ufcs_methods);
        let module_id = build.module_id;
        store.insert_prebuilt_module(module_id.clone(), build.module, build.file_map);
        checker.collect_cached_module_tests(store, &module_id);
        result.cached.insert(module_id);
    }

    result
}

fn register_modules(
    checker: &mut TaskState,
    store: &mut Store,
    to_infer: &[String],
    edges: &HashMap<String, HashSet<String>>,
) {
    if to_infer.len() < PARALLEL_THRESHOLD {
        for module_id in to_infer {
            checker.register_module(store, module_id);
        }
        return;
    }

    // Same-wave modules never read each other, so each worker mutates only its
    // own detached module and reads the rest through a snapshot.
    for wave in registration_waves(to_infer, edges) {
        if wave.len() == 1 {
            checker.register_module(store, &wave[0]);
            continue;
        }

        let detached: Vec<(String, Arc<Module>)> = wave
            .into_iter()
            .map(|module_id| {
                let module = store
                    .modules
                    .remove(&module_id)
                    .expect("fresh module must be stored before registration");
                (module_id, module)
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
                for (module_id, module) in chunk {
                    view.modules.insert(module_id.clone(), module);
                    worker.register_module(&mut view, &module_id);
                    let module = view
                        .modules
                        .remove(&module_id)
                        .expect("registered module must remain in view");
                    registered.push((module_id, module));
                }
                RegistrationOutput {
                    modules: registered,
                    task: worker.into_output(),
                }
            })
            .collect();

        let mut task_outputs = Vec::with_capacity(outputs.len());
        for output in outputs {
            for (module_id, module) in output.modules {
                store.modules.insert(module_id, module);
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

    let module_files: Vec<(String, Vec<File>)> = to_infer
        .iter()
        .map(|module_id| {
            let files = checker.take_module_files(store, module_id);
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

    for (_, typed_file) in std::mem::take(&mut checker.typed_files) {
        store.store_file(typed_file);
    }

    checker.check_post_inference_bounds(store);
}

/// Groups topologically ordered modules into dependency waves, so a wave only
/// reads modules registered in earlier waves.
fn registration_waves(
    modules: &[String],
    edges: &HashMap<String, HashSet<String>>,
) -> Vec<Vec<String>> {
    let mut wave_of: HashMap<&str, usize> = HashMap::default();
    let mut waves: Vec<Vec<String>> = Vec::new();
    for module_id in modules {
        let wave = edges
            .get(module_id)
            .into_iter()
            .flatten()
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
