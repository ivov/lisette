use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use diagnostics::{LocalSink, SemanticResult, TypedefSource};
use syntax::ast::Expression;
use syntax::program::{File, ModuleInfo, MutationInfo, UnusedInfo};

use deps::TypedefLocator;

use crate::cache::{
    CompiledModule, compute_module_hash, get_dependency_module_hashes,
    go_stdlib::{self, load_cached_go_module},
    hash_module_sources, is_cache_disabled, prelude as prelude_cache, register_cached_module,
    save_module_cache, try_load_cache,
};
use crate::checker::TaskState;
use crate::facts::{BindingIdAllocator, Facts};
use crate::loader::Loader;
use crate::module_graph::build_module_graph;
use crate::prelude::parse_and_register_prelude;
use crate::store::{ENTRY_MODULE_ID, Store};
use crate::validators;

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
    pub filename: String,
    pub ast: Vec<Expression>,
    pub project_root: Option<PathBuf>,
    pub compile_phase: CompilePhase,
    pub locator: TypedefLocator,
}

pub fn analyze(input: AnalyzeInput) -> (SemanticResult, Facts) {
    let mut store = Store::new();

    store.init_entry_module();
    store.store_entry_file(&input.filename, &input.source, input.ast);

    let sink = LocalSink::new();

    if input.config.load_siblings {
        for (filename, source) in input.loader.scan_folder(ENTRY_MODULE_ID) {
            if filename == input.filename
                || !filename.ends_with(".lis")
                || filename.ends_with(".d.lis")
            {
                continue;
            }
            let file_id = store.new_file_id();
            let result = syntax::build_ast(&source, file_id);
            sink.extend_parse_errors(result.errors);
            store.store_file(
                ENTRY_MODULE_ID,
                File::new(ENTRY_MODULE_ID, &filename, &source, result.ast, file_id),
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

    let cache_enabled = input.project_root.is_some() && !cache_disabled;
    let check_go_files = input.compile_phase == CompilePhase::Emit;

    let binding_ids = Arc::new(BindingIdAllocator::new());

    let (mut facts, cached_modules, compiled_modules, ufcs_methods) = {
        let mut checker = TaskState::new(&sink, binding_ids.clone());
        checker
            .ufcs_methods
            .extend(crate::prelude::compute_prelude_ufcs(&store));

        let mut module_hashes: HashMap<String, u64> = HashMap::default();
        let mut cached_modules: HashSet<String> = HashSet::default();
        let mut compiled_modules: Vec<CompiledModule> = vec![];

        let order = std::mem::take(&mut graph_result.order);
        let edges = &graph_result.edges;

        let go_cache = if cache_disabled {
            None
        } else {
            go_stdlib::try_load_go_stdlib_cache()
        };

        let mut to_infer: Vec<String> = Vec::new();

        for module_id in order {
            if let Some(go_pkg) = module_id.strip_prefix("go:") {
                if deps::is_stdlib(go_pkg)
                    && let Some(ref cache) = go_cache
                {
                    load_cached_go_module(&mut store, &module_id, cache);
                    if store.is_visited(&module_id) {
                        continue;
                    }
                }

                if let deps::TypedefLocatorResult::Found {
                    content: source, ..
                } = input.locator.find_typedef_content(go_pkg)
                {
                    checker.parse_and_register_go_module(
                        &mut store,
                        &module_id,
                        &source,
                        &input.locator,
                    );
                }
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

            if cache_enabled
                && !is_entry
                && let Some(ref project_root) = input.project_root
                && let Some(cached) = try_load_cache(
                    &module_id,
                    source_hash,
                    &dep_hashes,
                    project_root,
                    check_go_files,
                )
            {
                checker
                    .ufcs_methods
                    .extend(cached.ufcs_methods.iter().cloned());
                register_cached_module(&mut store, &module_id, cached);
                cached_modules.insert(module_id.clone());
                continue;
            }

            let prev_module_id = checker.cursor.module_id.clone();
            checker.cursor.module_id = module_id.to_string();

            store.store_module(&module_id, files);
            checker.register_module(&mut store, &module_id);

            checker.cursor.module_id = prev_module_id;

            if cache_enabled && !is_entry {
                compiled_modules.push(CompiledModule {
                    module_id: module_id.clone(),
                    source_hash,
                    dep_hashes,
                });
            }

            to_infer.push(module_id);
        }

        for module_id in &to_infer {
            let prev_module_id = checker.cursor.module_id.clone();
            checker.cursor.module_id = module_id.clone();

            checker.infer_module(&mut store, module_id);

            checker.cursor.module_id = prev_module_id;
        }

        for (module_id, typed_file) in std::mem::take(&mut checker.typed_files) {
            store.store_file(&module_id, typed_file);
        }

        // Save Go stdlib cache if store has Go modules not already in cache
        if !cache_disabled {
            let all_go_modules: Vec<String> = store
                .modules
                .keys()
                .filter(|id| id.strip_prefix("go:").is_some_and(deps::is_stdlib))
                .cloned()
                .collect();
            let needs_save = !all_go_modules.is_empty()
                && go_cache.as_ref().is_none_or(|c| {
                    all_go_modules.len() != c.modules.len()
                        || all_go_modules.iter().any(|id| !c.modules.contains_key(id))
                });
            if needs_save {
                go_stdlib::save_go_stdlib_cache(&store, &all_go_modules);
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

    let analysis = crate::context::AnalysisContext::new(&store, &ufcs_methods);

    let mut unused = UnusedInfo::default();
    if !has_pre_check_errors {
        validators::run(
            &analysis,
            &mut facts,
            &sink,
            &mut unused,
            input.config.run_lints,
        );
    }

    let mut mutations = MutationInfo::default();
    for (&binding_id, b) in facts.bindings.iter() {
        if b.mutated {
            mutations.mark_binding_mutated(binding_id);
        }
    }

    let (errors, lints): (Vec<_>, Vec<_>) = sink.take().into_iter().partition(|d| d.is_error());

    if cache_enabled && let Some(ref project_root) = input.project_root {
        let has_errors = errors.iter().any(|e| e.is_error());
        if !has_errors {
            for compiled in compiled_modules {
                let file_ids: HashSet<u32> = store
                    .get_module(&compiled.module_id)
                    .map(|m| m.file_ids().collect())
                    .unwrap_or_default();

                let has_module_warnings = lints.iter().any(|lint| {
                    lint.file_id()
                        .map(|fid| file_ids.contains(&fid))
                        .unwrap_or(false)
                });
                if !has_module_warnings
                    && let Err(e) =
                        save_module_cache(&compiled, &store, project_root, &ufcs_methods)
                {
                    eprintln!(
                        "warning: failed to write cache for {}: {e}",
                        compiled.module_id
                    );
                }
            }
        }
    }

    let mut files = HashMap::default();
    let mut definitions = HashMap::default();
    let mut modules = HashMap::default();
    let mut typedef_sources = HashMap::default();

    for (mod_id, module) in store.modules {
        let is_internal = module.is_internal();

        definitions.extend(module.definitions);

        if is_internal {
            typedef_sources.extend(module.typedefs.into_iter().map(|(id, file)| {
                (
                    id,
                    TypedefSource {
                        source: file.source,
                        filename: file.name,
                    },
                )
            }));
            continue;
        }

        modules.insert(
            mod_id,
            ModuleInfo {
                file_ids: module.files.keys().copied().collect(),
                typedef_ids: module.typedefs.keys().copied().collect(),
                id: module.id.clone(),
                path: module.id,
            },
        );

        files.extend(module.files);
        files.extend(module.typedefs);
    }

    let result = SemanticResult {
        files,
        definitions,
        modules,
        errors,
        lints,
        entry_module_id: ENTRY_MODULE_ID.to_string(),
        unused,
        mutations,
        cached_modules,
        ufcs_methods,
        typedef_sources,
        go_package_names: store.go_package_names,
    };

    (result, facts)
}
