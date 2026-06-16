use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use diagnostics::LocalSink;
use ecow::EcoString;
use syntax::ast::{Expression, StructFieldDefinition};
use syntax::program::{File, Module};

use deps::TypedefLocator;

use crate::cache::{
    CompiledModule, compute_emit_artifact_hash, compute_module_hash, get_dependency_module_hashes,
    go_stdlib::{self, load_cached_go_module},
    hash_module_sources, is_cache_disabled, prelude as prelude_cache, register_cached_module,
    try_load_cache,
};
use crate::checker::TaskState;
use crate::checker::infer::InferCtx;
use crate::diagnostics::emit_for_locator_result;
use crate::facts::{BindingIdAllocator, Facts};
use crate::loader::Loader;
use crate::module_graph::build_module_graph;
use crate::prelude::parse_and_register_prelude;
use crate::store::{ENTRY_MODULE_ID, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilePhase {
    #[default]
    Check,
    Emit,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticConfig {
    pub run_lints: bool,
    pub standalone_mode: bool,
    pub load_siblings: bool,
}

pub struct AnalyzeInput<'a> {
    pub config: SemanticConfig,
    pub loader: &'a dyn Loader,
    pub source: String,
    /// Bare identity name of the entry file (e.g. `main.lis`).
    pub filename: String,
    /// Cwd-relative display path for the entry file (e.g. `src/main.lis`);
    /// equals `filename` when there is no separate display path.
    pub display_path: String,
    pub ast: Vec<Expression>,
    pub project_root: Option<PathBuf>,
    pub compile_phase: CompilePhase,
    pub locator: TypedefLocator,
    /// Go module path (from `lisette.toml`); folded into the cache emit-artifact
    /// hash so a project rename invalidates Go outputs.
    pub go_module: String,
    /// When true, `analyze` skips both cache load and save. Set by the CLI for
    /// `--sourcemap` Emit so cwd-decorated Go files are not reused across cwds.
    pub disable_cache: bool,
}

// Tiny projects stay serial to avoid rayon overhead.
const PARALLEL_THRESHOLD: usize = 4;

pub struct InferenceOutput {
    pub store: Store,
    pub facts: Facts,
    pub ufcs_methods: HashSet<(String, String)>,
    pub sink: LocalSink,
    pub has_pre_check_errors: bool,
    pub compiled_modules: Vec<CompiledModule>,
    pub cached_modules: HashSet<String>,
    pub cache_enabled: bool,
}

/// Loads, registers, and infers every module, returning the artifacts the
/// post-inference passes consume. Internal, unstable API.
pub fn run_inference(input: AnalyzeInput) -> InferenceOutput {
    let mut store = Store::new();

    store.init_entry_module();
    store.store_entry_file(
        &input.filename,
        &input.display_path,
        &input.source,
        input.ast,
    );

    let sink = LocalSink::new();

    if input.config.load_siblings {
        for (filename, content) in input.loader.scan_folder(ENTRY_MODULE_ID) {
            if filename == input.filename
                || !filename.ends_with(".lis")
                || filename.ends_with(".d.lis")
            {
                continue;
            }
            let file_id = store.new_file_id();
            let result = syntax::build_ast(&content.source, file_id);
            sink.extend_parse_errors(result.errors);
            store.store_file(
                ENTRY_MODULE_ID,
                File::new(
                    ENTRY_MODULE_ID,
                    &filename,
                    &content.display_path,
                    &content.source,
                    result.ast,
                    file_id,
                ),
            );
        }
    }

    let entry_module = store.entry_module_id().to_string();
    let mut graph_result = build_module_graph(
        &mut store,
        Some(input.loader),
        &entry_module,
        &sink,
        input.config.standalone_mode,
        &input.locator,
    );

    for cycle in &graph_result.cycles {
        sink.push(diagnostics::module_graph::import_cycle(cycle));
    }

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

    let cache_enabled = input.project_root.is_some() && !cache_disabled && !input.disable_cache;
    let check_go_files = input.compile_phase == CompilePhase::Emit;

    let binding_ids = Arc::new(BindingIdAllocator::new());

    let (facts, cached_modules, compiled_modules, ufcs_methods) = {
        let mut checker = TaskState::new(&sink, binding_ids.clone());
        checker
            .ufcs_methods
            .extend(crate::prelude::compute_prelude_ufcs(&store));

        let mut module_hashes: HashMap<String, u64> = HashMap::default();
        let mut cached_modules: HashSet<String> = HashSet::default();
        let mut compiled_modules: Vec<CompiledModule> = vec![];

        let order = std::mem::take(&mut graph_result.order);
        let edges = &graph_result.edges;

        // Outer `None` = not attempted: the deserialize costs milliseconds a
        // project without stdlib imports should not pay.
        let mut go_cache: Option<Option<go_stdlib::GoStdlibCache>> =
            if cache_disabled { Some(None) } else { None };

        let mut to_infer: Vec<String> = Vec::new();

        for module_id in order {
            if module_id.starts_with("go:") {
                if graph_result.link_only_modules.contains(&module_id) {
                    continue;
                }
                register_go_module(
                    &mut checker,
                    &mut store,
                    &sink,
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
            let source_hash = hash_module_sources(&files);

            let dep_hashes = get_dependency_module_hashes(&module_id, edges, &module_hashes);
            let module_hash = compute_module_hash(source_hash, &dep_hashes);
            module_hashes.insert(module_id.clone(), module_hash);

            let is_entry = module_id == ENTRY_MODULE_ID;

            let expected_artifact_hash =
                check_go_files.then(|| compute_emit_artifact_hash(source_hash, &input.go_module));

            if cache_enabled
                && !is_entry
                && let Some(ref project_root) = input.project_root
                && let Some(cached) = try_load_cache(
                    &module_id,
                    source_hash,
                    &dep_hashes,
                    expected_artifact_hash,
                    project_root,
                    check_go_files,
                )
            {
                checker
                    .ufcs_methods
                    .extend(cached.ufcs_methods.iter().cloned());
                register_cached_module(&mut store, &module_id, cached, project_root);
                cached_modules.insert(module_id.clone());
                continue;
            }

            store.store_module(&module_id, files);

            if !is_entry {
                compiled_modules.push(CompiledModule {
                    module_id: module_id.clone(),
                    source_hash,
                    dep_hashes,
                });
            }

            to_infer.push(module_id);
        }

        register_modules(
            &mut checker,
            &mut store,
            &sink,
            &to_infer,
            edges,
            &binding_ids,
        );
        infer_modules(&mut checker, &mut store, &sink, &to_infer, &binding_ids);

        if !cache_disabled {
            let all_go_modules: Vec<String> = store
                .modules
                .keys()
                .filter(|id| id.strip_prefix("go:").is_some_and(deps::is_stdlib))
                .cloned()
                .collect();
            // A non-empty list implies the lazy cache load was attempted.
            let needs_save = !all_go_modules.is_empty()
                && go_cache.as_ref().and_then(Option::as_ref).is_none_or(|c| {
                    all_go_modules.len() != c.modules.len()
                        || all_go_modules.iter().any(|id| !c.modules.contains_key(id))
                });
            if needs_save {
                go_stdlib::save_go_stdlib_cache(&store, &all_go_modules, input.locator.target());
            }
        }

        if !cache_disabled && !prelude_cache_hit {
            prelude_cache::save_prelude_cache(&store);
        }

        (
            checker.facts,
            cached_modules,
            compiled_modules,
            checker.ufcs_methods,
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
    }
}

/// Registers one `go:` module, reusing the stdlib cache when it covers the package.
fn register_go_module(
    checker: &mut TaskState,
    store: &mut Store,
    sink: &LocalSink,
    module_id: &str,
    locator: &TypedefLocator,
    standalone_mode: bool,
    go_cache: &mut Option<Option<go_stdlib::GoStdlibCache>>,
) {
    let go_pkg = module_id.strip_prefix("go:").unwrap_or(module_id);
    if deps::is_stdlib(go_pkg)
        && let Some(ref cache) =
            *go_cache.get_or_insert_with(|| go_stdlib::try_load_go_stdlib_cache(locator.target()))
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
                module_id,
                go_pkg,
                None,
                locator.target(),
                standalone_mode,
                sink,
            );
        }
    }
}

fn register_modules(
    checker: &mut TaskState,
    store: &mut Store,
    sink: &LocalSink,
    to_infer: &[String],
    edges: &HashMap<String, HashSet<String>>,
    binding_ids: &Arc<BindingIdAllocator>,
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
        let mut chunks: Vec<Vec<(String, Arc<Module>)>> = Vec::new();
        let mut remaining = detached.into_iter();
        loop {
            let chunk: Vec<_> = remaining.by_ref().take(chunk_size).collect();
            if chunk.is_empty() {
                break;
            }
            chunks.push(chunk);
        }

        let allocator = binding_ids.clone();
        let store_ref: &Store = store;
        let fields_shared = Arc::new(checker.module_fields_snapshot());

        type RegisterOutput = (
            Vec<(String, Arc<Module>)>,
            HashSet<(String, String)>,
            HashMap<EcoString, Arc<[StructFieldDefinition]>>,
            Facts,
            LocalSink,
        );
        let outputs: Vec<RegisterOutput> = chunks
            .into_par_iter()
            .map(|chunk| {
                let local_sink = LocalSink::new();
                let mut worker = TaskState::new(&local_sink, allocator.clone());
                worker.module_fields_shared = Some(fields_shared.clone());
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
                let facts = std::mem::replace(&mut worker.facts, Facts::new(allocator.clone()));
                (
                    registered,
                    std::mem::take(&mut worker.ufcs_methods),
                    worker.module_fields_snapshot(),
                    facts,
                    local_sink,
                )
            })
            .collect();

        let mut worker_sinks: Vec<LocalSink> = Vec::with_capacity(outputs.len());
        for (registered, ufcs_methods, module_fields, facts, sink_local) in outputs {
            for (module_id, module) in registered {
                store.modules.insert(module_id, module);
            }
            checker.ufcs_methods.extend(ufcs_methods);
            checker.merge_module_fields(module_fields);
            checker.facts.merge(facts);
            worker_sinks.push(sink_local);
        }
        sink.extend(LocalSink::merge(worker_sinks));
    }
}

fn infer_modules(
    checker: &mut TaskState,
    store: &mut Store,
    sink: &LocalSink,
    to_infer: &[String],
    binding_ids: &Arc<BindingIdAllocator>,
) {
    checker.finalize_equality(store);

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
        let allocator = binding_ids.clone();
        let ufcs_shared = Arc::new(std::mem::take(&mut checker.ufcs_methods));
        let fields_shared = Arc::new(checker.module_fields_snapshot());
        let store_ref: &Store = store;

        type WorkerOutput = (Vec<(String, File)>, Facts, LocalSink);
        let outputs: Vec<WorkerOutput> = module_files
            .into_par_iter()
            .map(|(module_id, files)| {
                let local_sink = LocalSink::new();
                let mut worker = TaskState::new(&local_sink, allocator.clone());
                worker.ufcs_shared = Some(ufcs_shared.clone());
                worker.module_fields_shared = Some(fields_shared.clone());
                InferCtx::new(&mut worker, store_ref).infer_module(&module_id, files);
                let typed_files = std::mem::take(&mut worker.typed_files);
                let facts = std::mem::replace(&mut worker.facts, Facts::new(allocator.clone()));
                (typed_files, facts, local_sink)
            })
            .collect();

        checker.ufcs_methods = Arc::try_unwrap(ufcs_shared).unwrap_or_else(|arc| (*arc).clone());

        let mut worker_sinks: Vec<LocalSink> = Vec::with_capacity(outputs.len());
        for (typed_files, facts, sink_local) in outputs {
            checker.typed_files.extend(typed_files);
            checker.facts.merge(facts);
            worker_sinks.push(sink_local);
        }
        sink.extend(LocalSink::merge(worker_sinks));
    }

    for (module_id, typed_file) in std::mem::take(&mut checker.typed_files) {
        store.store_file(&module_id, typed_file);
    }
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
