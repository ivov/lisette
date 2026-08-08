use super::*;
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use syntax::program::UninferredExports;

struct CacheCandidate {
    pending: CompiledPendingPackage,
    files: Vec<ScannedFile>,
    rewrite_root_import: bool,
}

struct UnparsedPackage {
    files: Vec<ScannedFile>,
    rewrite_root_import: bool,
    pending: PendingPackage,
}

struct ParsedPackage {
    files: Vec<File>,
    errors: Vec<ParseError>,
    pending: PendingPackage,
}

impl UnparsedPackage {
    fn parse(self) -> ParsedPackage {
        let Self {
            files: scanned,
            rewrite_root_import,
            pending,
        } = self;

        let package_id = pending.package_id();
        let mut files = Vec::with_capacity(scanned.len());
        let mut errors = Vec::new();
        for scanned_file in scanned {
            let (mut file, file_errors) = scanned_file.parse(package_id);
            if rewrite_root_import {
                file.rewrite_import(crate::loader::ROOT_IMPORT, ENTRY_PACKAGE_ID);
            }
            files.push(file);
            errors.extend(file_errors);
        }

        ParsedPackage {
            files,
            errors,
            pending,
        }
    }
}

struct CompiledPendingPackage {
    package: CompiledPackage,
    topo_rank: usize,
}

enum PendingPackage {
    Entry { topo_rank: usize },
    Compiled(CompiledPendingPackage),
}

impl PendingPackage {
    fn package_id(&self) -> &str {
        match self {
            Self::Entry { .. } => ENTRY_PACKAGE_ID,
            Self::Compiled(pending) => &pending.package.package_id,
        }
    }

    fn topo_rank(&self) -> usize {
        match self {
            Self::Entry { topo_rank } => *topo_rank,
            Self::Compiled(pending) => pending.topo_rank,
        }
    }
}

struct CacheBuildJob {
    package_id: String,
    interface: PackageInterface,
    file_id_base: u32,
}

struct RegistrationOutput {
    packages: Vec<Arc<Package>>,
    registered: Vec<RegisteredPackage>,
    task: TaskOutput,
}

pub(super) struct PackageInferenceInput<'a> {
    pub(super) order: Vec<String>,
    pub(super) files: HashMap<String, Vec<ScannedFile>>,
    pub(super) dependencies: DependencyGraph,
    pub(super) sink: LocalSink,
    pub(super) compile_phase: CompilePhase,
    pub(super) go_module: &'a str,
    pub(super) cache: CacheState<'a>,
    pub(super) locator: &'a TypedefLocator,
    pub(super) scope: &'a AnalysisScope,
    pub(super) project_kind: ProjectKind,
}

pub(super) struct PackageInferenceOutput {
    pub(super) facts: Facts,
    pub(super) cached_packages: HashSet<String>,
    pub(super) compiled_packages: Vec<CompiledPackage>,
    pub(super) sink: LocalSink,
    pub(super) has_parse_errors: bool,
}

/// Classifies every topo-ordered package as a `go:` import, a cache candidate,
/// or pending registration, then registers and infers whatever was not
/// served from cache.
pub(super) fn infer_all_packages(
    store: &mut Store,
    mut input: PackageInferenceInput,
) -> PackageInferenceOutput {
    let mut checker =
        TaskState::with_sink(input.sink, input.project_kind, input.scope.script_unit());

    let mut package_hashes: HashMap<String, u64> = HashMap::default();
    let mut cached_packages: HashSet<String> = HashSet::default();
    let order = input.order;
    let dependencies = &input.dependencies;

    let mut to_infer: Vec<PendingPackage> = Vec::new();
    let mut candidates: Vec<CacheCandidate> = Vec::new();
    let mut unparsed: Vec<UnparsedPackage> = Vec::new();

    let mut source_hashes: HashMap<String, (u64, u64)> = if input.files.len() < PARALLEL_THRESHOLD {
        input
            .files
            .iter()
            .map(|(id, files)| (id.clone(), hash_package_source_pair(scanned_sources(files))))
            .collect()
    } else {
        input
            .files
            .par_iter()
            .map(|(id, files)| (id.clone(), hash_package_source_pair(scanned_sources(files))))
            .collect()
    };

    let entry_files: Vec<&File> = store
        .get_package(ENTRY_PACKAGE_ID)
        .map(|package| package.files.values().collect())
        .unwrap_or_default();
    if !entry_files.is_empty() {
        let sources = entry_files
            .iter()
            .map(|file| (file.name.as_str(), file.source.as_str()));
        source_hashes.insert(
            ENTRY_PACKAGE_ID.to_string(),
            hash_package_source_pair(sources),
        );
    }

    for (topo_rank, package_id) in order.into_iter().enumerate() {
        if package_id.starts_with("go:") {
            if dependencies.is_link_only_package(&package_id) {
                continue;
            }
            register_go_package(
                &mut checker,
                store,
                &package_id,
                input.locator,
                input.scope.script_unit(),
                input.cache.go_stdlib_mut(),
            );
            continue;
        }

        let files = input.files.remove(&package_id).unwrap_or_default();
        let rewrite_root_import = input.scope.has_project_root()
            && input.project_kind == ProjectKind::Library
            && crate::loader::is_external_test_package(&package_id);
        // Production-only hash drives dependents/emit; all-files hash drives own validity.
        let (production_hash, full_hash) = source_hashes
            .get(&package_id)
            .copied()
            .unwrap_or_else(|| hash_package_source_pair(scanned_sources(&files)));

        let dep_hashes =
            get_dependency_package_hashes(dependencies.dependencies(&package_id), &package_hashes);
        let production_dep_hashes = get_dependency_package_hashes(
            dependencies.production_dependencies(&package_id),
            &package_hashes,
        );
        let package_hash = compute_package_hash(production_hash, &production_dep_hashes);
        package_hashes.insert(package_id.clone(), package_hash);

        let is_entry = package_id == ENTRY_PACKAGE_ID;

        let compiled = (!is_entry).then(|| CompiledPackage {
            package_id: package_id.clone(),
            artifact_hash: compute_emit_artifact_hash(production_hash, input.go_module),
            full_hash,
            dep_hashes,
        });

        match (input.cache.package_root(), compiled) {
            (Some(_), Some(compiled)) => candidates.push(CacheCandidate {
                pending: CompiledPendingPackage {
                    package: compiled,
                    topo_rank,
                },
                files,
                rewrite_root_import,
            }),
            (None, compiled) | (Some(_), compiled @ None) => {
                let pending = match compiled {
                    Some(package) => {
                        PendingPackage::Compiled(CompiledPendingPackage { package, topo_rank })
                    }
                    None => PendingPackage::Entry { topo_rank },
                };
                unparsed.push(UnparsedPackage {
                    files,
                    rewrite_root_import,
                    pending,
                });
            }
        }
    }

    let go_cache_package_ids = input.cache.go_package_ids();

    let cache_load = match input.cache.package_root() {
        Some(root) => {
            load_cache_candidates(&mut checker, store, candidates, root, input.compile_phase)
        }
        None => {
            debug_assert!(candidates.is_empty());
            CacheLoad::default()
        }
    };
    cached_packages.extend(cache_load.cached);
    unparsed.extend(cache_load.missed);

    let has_parse_errors = parse_and_store_packages(&mut checker, store, unparsed, &mut to_infer);
    let uninferred = input.files;
    store_uninferred_packages(&mut checker, store, uninferred);

    for pending in &to_infer {
        checker.predeclare_package_types(store, pending.package_id());
    }
    to_infer.sort_by_key(PendingPackage::topo_rank);
    let mut compiled_packages = Vec::new();
    let to_infer: Vec<String> = to_infer
        .into_iter()
        .map(|pending| match pending {
            PendingPackage::Entry { .. } => ENTRY_PACKAGE_ID.to_string(),
            PendingPackage::Compiled(pending) => {
                let package_id = pending.package.package_id.clone();
                compiled_packages.push(pending.package);
                package_id
            }
        })
        .collect();

    let unregistered = to_infer
        .iter()
        .map(|package_id| TaskState::take_unregistered_package(store, package_id))
        .collect();
    let registered = register_packages(&mut checker, store, unregistered, dependencies);
    infer_packages(&mut checker, store, registered);

    if !input.cache.is_disabled() {
        let all_go_packages: Vec<String> = store
            .packages
            .keys()
            .filter(|id| id.strip_prefix("go:").is_some_and(deps::is_stdlib))
            .cloned()
            .collect();
        // A non-empty list implies the lazy cache load was attempted.
        let needs_save = !all_go_packages.is_empty()
            && go_cache_package_ids.as_ref().is_none_or(|ids| {
                all_go_packages.len() != ids.len()
                    || all_go_packages.iter().any(|id| !ids.contains(id))
            });
        if needs_save {
            go_stdlib::save_go_stdlib_cache(store, &all_go_packages, input.locator.target());
        }
    }

    if input.cache.should_save_prelude() {
        prelude_cache::save_prelude_cache(store);
    }

    PackageInferenceOutput {
        facts: checker.facts,
        cached_packages,
        compiled_packages,
        sink: checker.sink,
        has_parse_errors,
    }
}

fn scanned_sources(files: &[ScannedFile]) -> impl Iterator<Item = (&str, &str)> + Clone {
    files
        .iter()
        .map(|file| (file.name.as_str(), file.source.as_str()))
}

/// Parses everything the cache did not serve, in one batch.
fn parse_and_store_packages(
    checker: &mut TaskState,
    store: &mut Store,
    unparsed: Vec<UnparsedPackage>,
    to_infer: &mut Vec<PendingPackage>,
) -> bool {
    let file_count: usize = unparsed.iter().map(|package| package.files.len()).sum();
    let parsed: Vec<ParsedPackage> = if file_count < PARALLEL_THRESHOLD {
        unparsed.into_iter().map(UnparsedPackage::parse).collect()
    } else {
        unparsed
            .into_par_iter()
            .map(UnparsedPackage::parse)
            .collect()
    };

    let mut has_parse_errors = false;
    for package in parsed {
        has_parse_errors |= !package.errors.is_empty();
        checker.sink.extend_parse_errors(package.errors);
        store.store_package(package.pending.package_id(), package.files);
        to_infer.push(package.pending);
    }
    has_parse_errors
}

fn store_uninferred_packages(
    checker: &mut TaskState,
    store: &mut Store,
    packages: HashMap<String, Vec<ScannedFile>>,
) {
    for (package_id, scanned) in packages {
        if scanned.is_empty() {
            continue;
        }
        let mut files = Vec::with_capacity(scanned.len());
        let mut parsed = true;
        for scanned_file in scanned {
            let (file, errors) = scanned_file.parse(&package_id);
            parsed &= errors.is_empty();
            checker.sink.extend_parse_errors(errors);
            files.push(file);
        }
        let exports = if parsed {
            UninferredExports::Known(
                files
                    .iter()
                    .filter(|file| !file.is_test())
                    .flat_map(File::public_declarations)
                    .collect(),
            )
        } else {
            UninferredExports::Unreadable
        };
        store.store_uninferred_package(&package_id, files, exports);
    }
}

/// Registers one `go:` package, reusing the stdlib cache when it covers the package.
fn register_go_package(
    checker: &mut TaskState,
    store: &mut Store,
    package_id: &str,
    locator: &TypedefLocator,
    script: Option<ScriptUnit>,
    go_cache: Option<&mut LazyGoStdlibCache>,
) {
    let go_pkg = package_id.strip_prefix("go:").unwrap_or(package_id);
    if deps::is_stdlib(go_pkg)
        && let Some(cache) = go_cache.and_then(|cache| cache.get_or_load(locator.target()))
    {
        load_cached_go_package(store, package_id, cache, locator.target());
        if store.has(package_id) {
            return;
        }
    }

    match locator.find_typedef_content(go_pkg) {
        deps::TypedefLocatorResult::Found { content, origin } => {
            checker.parse_and_register_go_package(
                store,
                package_id,
                content.as_ref(),
                origin.into_cache_path(),
                locator,
            );
        }
        other => {
            emit_for_locator_result(
                &other,
                &GoImportSite {
                    go_pkg,
                    name_span: None,
                    target: locator.target(),
                    script,
                    replace_importer: None,
                    transitive_importer: None,
                },
                &checker.sink,
            );
        }
    }
}

#[derive(Default)]
struct CacheLoad {
    cached: HashSet<String>,
    missed: Vec<UnparsedPackage>,
}

/// Merges cache hits into the store and returns the misses, still unparsed.
fn load_cache_candidates(
    checker: &mut TaskState,
    store: &mut Store,
    candidates: Vec<CacheCandidate>,
    project_root: &Path,
    compile_phase: CompilePhase,
) -> CacheLoad {
    let load = |c: &CacheCandidate| {
        let compiled = &c.pending.package;
        let expected_artifact_hash = compile_phase.emits().then_some(compiled.artifact_hash);
        try_load_cache(
            &compiled.package_id,
            compiled.full_hash,
            &compiled.dep_hashes,
            expected_artifact_hash,
            project_root,
        )
    };
    let loaded: Vec<Option<PackageInterface>> = if candidates.len() < PARALLEL_THRESHOLD {
        candidates.iter().map(load).collect()
    } else {
        candidates.par_iter().map(load).collect()
    };

    let mut result = CacheLoad::default();
    let mut build_jobs: Vec<CacheBuildJob> = Vec::new();
    for (candidate, interface) in candidates.into_iter().zip(loaded) {
        let Some(interface) = interface else {
            result.missed.push(UnparsedPackage {
                files: candidate.files,
                rewrite_root_import: candidate.rewrite_root_import,
                pending: PendingPackage::Compiled(candidate.pending),
            });
            continue;
        };
        let file_id_base = store.reserve_file_ids(interface.files.len() as u32);
        build_jobs.push(CacheBuildJob {
            package_id: candidate.pending.package.package_id,
            interface,
            file_id_base,
        });
    }

    let src_base = crate::path::DisplayPathBase::new(&project_root.join("src"));
    let root_base = crate::path::DisplayPathBase::new(project_root);
    let build = |job: CacheBuildJob| {
        build_cached_package(
            job.package_id,
            job.file_id_base,
            job.interface,
            &src_base,
            &root_base,
        )
    };
    let built: Vec<Package> = if build_jobs.len() < PARALLEL_THRESHOLD {
        build_jobs.into_iter().map(build).collect()
    } else {
        build_jobs.into_par_iter().map(build).collect()
    };

    for package in built {
        let package_id = package.id.clone();
        store.insert_prebuilt_package(package);
        checker.collect_cached_package_tests(store, &package_id);
        result.cached.insert(package_id);
    }

    result
}

fn register_packages(
    checker: &mut TaskState,
    store: &mut Store,
    packages: Vec<UnregisteredPackage>,
    dependencies: &DependencyGraph,
) -> Vec<RegisteredPackage> {
    if packages.len() < PARALLEL_THRESHOLD {
        return packages
            .into_iter()
            .map(|package| checker.register_predeclared_package(store, package))
            .collect();
    }

    let package_ids: Vec<_> = packages.iter().map(|package| package.id.clone()).collect();
    let mut inputs: HashMap<_, _> = packages
        .into_iter()
        .map(|package| (package.id.clone(), package))
        .collect();
    let mut all_registered = Vec::with_capacity(package_ids.len());

    // Same-wave packages never read each other, so each worker mutates only its
    // own detached package and reads the rest through a snapshot.
    for wave in registration_waves(&package_ids, dependencies) {
        if wave.len() == 1 {
            let input = inputs
                .remove(&wave[0])
                .expect("registration input must match its package");
            all_registered.push(checker.register_predeclared_package(store, input));
            continue;
        }

        let detached: Vec<(Arc<Package>, UnregisteredPackage)> = wave
            .into_iter()
            .map(|package_id| {
                let package = store
                    .packages
                    .remove(&package_id)
                    .expect("fresh package must be stored before registration");
                let input = inputs
                    .remove(&package_id)
                    .expect("registration input must match its package");
                (package, input)
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
                let mut registered_packages = Vec::with_capacity(chunk.len());
                let mut registered_inputs = Vec::with_capacity(chunk.len());
                for (package, input) in chunk {
                    let package_id = package.id.clone();
                    view.packages.insert(package_id.clone(), package);
                    registered_inputs.push(worker.register_predeclared_package(&mut view, input));
                    let package = view
                        .packages
                        .remove(&package_id)
                        .expect("registered package must remain in view");
                    registered_packages.push(package);
                }
                RegistrationOutput {
                    packages: registered_packages,
                    registered: registered_inputs,
                    task: worker.into_output(),
                }
            })
            .collect();

        let mut task_outputs = Vec::with_capacity(outputs.len());
        for output in outputs {
            for package in output.packages {
                store.packages.insert(package.id.clone(), package);
            }
            all_registered.extend(output.registered);
            task_outputs.push(output.task);
        }
        checker.absorb_outputs(task_outputs);
    }
    debug_assert!(inputs.is_empty());
    all_registered
}

fn infer_packages(checker: &mut TaskState, store: &mut Store, packages: Vec<RegisteredPackage>) {
    checker.finalize_registration(store);

    let inferred_files = if packages.len() < PARALLEL_THRESHOLD {
        let mut inferred_files = Vec::new();
        for package in packages {
            inferred_files.extend(InferCtx::new(checker, store).infer_package(package));
        }
        inferred_files
    } else {
        let seed = checker.worker_seed();
        let store_ref: &Store = store;

        let outputs: Vec<(TaskOutput, Vec<crate::checker::InferredFile>)> = packages
            .into_par_iter()
            .map(|package| {
                let mut worker = seed.spawn();
                let inferred_files = InferCtx::new(&mut worker, store_ref).infer_package(package);
                (worker.into_output(), inferred_files)
            })
            .collect();

        let (task_outputs, inferred_files): (Vec<_>, Vec<_>) = outputs.into_iter().unzip();
        checker.absorb_outputs(task_outputs);
        inferred_files.into_iter().flatten().collect()
    };

    TaskState::install_inferred_files(store, inferred_files);

    checker.check_post_inference_bounds(store);
}

/// Groups topologically ordered packages into dependency waves, so a wave only
/// reads packages registered in earlier waves.
fn registration_waves(packages: &[String], dependencies: &DependencyGraph) -> Vec<Vec<String>> {
    let mut wave_of: HashMap<&str, usize> = HashMap::default();
    let mut waves: Vec<Vec<String>> = Vec::new();
    for package_id in packages {
        let wave = dependencies
            .dependencies(package_id)
            .filter_map(|dep| wave_of.get(dep.as_str()))
            .map(|dep_wave| dep_wave + 1)
            .max()
            .unwrap_or(0);
        wave_of.insert(package_id, wave);
        if waves.len() == wave {
            waves.push(Vec::new());
        }
        waves[wave].push(package_id.clone());
    }
    waves
}
