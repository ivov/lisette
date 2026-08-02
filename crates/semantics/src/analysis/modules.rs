use super::*;
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;

struct CacheCandidate {
    compiled: CompiledModule,
    files: Vec<File>,
    topo_rank: usize,
}

enum PendingModule {
    Entry {
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
            Self::Entry { .. } => ENTRY_MODULE_ID,
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

pub(super) struct ModuleInferenceInput<'a> {
    pub(super) graph_result: crate::module_graph::ModuleGraphResult,
    pub(super) sink: LocalSink,
    pub(super) compile_phase: CompilePhase,
    pub(super) go_module: &'a str,
    pub(super) cache: CacheState<'a>,
    pub(super) locator: &'a TypedefLocator,
    pub(super) scope: &'a AnalysisScope,
    pub(super) project_kind: ProjectKind,
}

pub(super) struct ModuleInferenceOutput {
    pub(super) facts: Facts,
    pub(super) cached_modules: HashSet<String>,
    pub(super) compiled_modules: Vec<CompiledModule>,
    pub(super) sink: LocalSink,
}

/// Classifies every topo-ordered module as a `go:` import, a cache candidate,
/// or pending registration, then registers and infers whatever was not
/// served from cache.
pub(super) fn infer_all_modules(
    store: &mut Store,
    mut input: ModuleInferenceInput,
) -> ModuleInferenceOutput {
    let mut checker = TaskState::with_sink(input.sink, input.project_kind);

    let mut module_hashes: HashMap<String, u64> = HashMap::default();
    let mut cached_modules: HashSet<String> = HashSet::default();
    let order = std::mem::take(&mut input.graph_result.order);
    let dependencies = &input.graph_result.dependencies;

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
                input.scope.standalone_unit(),
                input.cache.go_stdlib_mut(),
            );
            continue;
        }

        let mut files = input
            .graph_result
            .files
            .remove(&module_id)
            .unwrap_or_default();
        if input.scope.has_project_root()
            && input.project_kind == ProjectKind::Library
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

        match (input.cache.module_root(), compiled) {
            (Some(_), Some(compiled)) => candidates.push(CacheCandidate {
                compiled,
                files,
                topo_rank,
            }),
            (None, compiled) | (Some(_), compiled @ None) => {
                store.store_module(&module_id, files);
                let pending = match compiled {
                    Some(module) => PendingModule::Compiled { module, topo_rank },
                    None => PendingModule::Entry { topo_rank },
                };
                to_infer.push(pending);
            }
        }
    }

    let go_cache_module_ids = input.cache.go_module_ids();

    let cache_load = match input.cache.module_root() {
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
    to_infer.sort_by_key(PendingModule::topo_rank);
    let mut compiled_modules = Vec::new();
    let to_infer: Vec<String> = to_infer
        .into_iter()
        .map(|pending| match pending {
            PendingModule::Entry { .. } => ENTRY_MODULE_ID.to_string(),
            PendingModule::Compiled { module, .. } => {
                let module_id = module.module_id.clone();
                compiled_modules.push(module);
                module_id
            }
        })
        .collect();

    register_modules(&mut checker, store, &to_infer, dependencies);
    infer_modules(&mut checker, store, &to_infer);

    if !input.cache.is_disabled() {
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

    if input.cache.should_save_prelude() {
        prelude_cache::save_prelude_cache(store);
    }

    ModuleInferenceOutput {
        facts: checker.facts,
        cached_modules,
        compiled_modules,
        sink: checker.sink,
    }
}

/// Registers one `go:` module, reusing the stdlib cache when it covers the package.
fn register_go_module(
    checker: &mut TaskState,
    store: &mut Store,
    module_id: &str,
    locator: &TypedefLocator,
    standalone: Option<StandaloneUnit>,
    go_cache: Option<&mut LazyGoStdlibCache>,
) {
    let go_pkg = module_id.strip_prefix("go:").unwrap_or(module_id);
    if deps::is_stdlib(go_pkg)
        && let Some(cache) = go_cache.and_then(|cache| cache.get_or_load(locator.target()))
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
                    standalone,
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
    let built: Vec<Module> = if build_jobs.len() < PARALLEL_THRESHOLD {
        build_jobs.into_iter().map(build).collect()
    } else {
        build_jobs.into_par_iter().map(build).collect()
    };

    for module in built {
        let module_id = module.id.clone();
        store.insert_prebuilt_module(module);
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
