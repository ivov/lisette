use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::PathBuf;

use diagnostics::{DiagnosticSink, SemanticResult};
use syntax::ast::Expression;
use syntax::program::{File, ModuleInfo, MutationInfo, UnusedInfo};

use crate::cache::{
    CompiledModule, compute_module_hash, get_dependency_module_hashes,
    go_stdlib::{self, load_cached_go_module},
    hash_module_sources, is_cache_disabled, prelude as prelude_cache, register_cached_module,
    save_module_cache, try_load_cache,
};
use crate::checker::Checker;
use crate::checker::infer::checks::check_interface_visibility;
use crate::facts::Facts;
use crate::lint;
use crate::loader::Loader;
use crate::module_graph::build_module_graph;
use crate::pattern_analysis;
use crate::prelude::parse_and_register_prelude;
use crate::store::{ENTRY_MODULE_ID, Store};
use stdlib::get_go_stdlib_typedef;

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
}

pub fn analyze(input: AnalyzeInput) -> (SemanticResult, Facts) {
    let mut store = Store::new();

    store.init_entry_module();
    store.store_entry_file(&input.filename, &input.source, input.ast);

    let sink = DiagnosticSink::new();

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

    let (mut facts, coercions, resolutions, cached_modules, compiled_modules, ufcs_methods) = {
        let mut checker = Checker::new(&mut store, &sink);
        checker
            .ufcs_methods
            .extend(crate::prelude::compute_prelude_ufcs(checker.store));

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

        for module_id in order {
            if let Some(go_pkg) = module_id.strip_prefix("go:") {
                if let Some(ref cache) = go_cache {
                    // Load this module + transitive deps from cache
                    load_cached_go_module(checker.store, &module_id, cache);
                    if checker.store.is_visited(&module_id) {
                        continue;
                    }
                }
                // Cache miss: parse and register
                if let Some(typedef) = get_go_stdlib_typedef(go_pkg) {
                    checker.parse_and_register_go_module(&module_id, typedef);
                }
                continue;
            }

            if checker.store.is_visited(&module_id) {
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
                register_cached_module(checker.store, &module_id, cached);
                cached_modules.insert(module_id.clone());
                continue;
            }

            let prev_module_id = checker.cursor.module_id.clone();
            checker.cursor.module_id = module_id.to_string();

            checker.store.store_module(&module_id, files);
            checker.register_module(&module_id);
            checker.infer_module(&module_id);
            check_interface_visibility(checker.store, &module_id, &sink);

            checker.cursor.module_id = prev_module_id;

            if cache_enabled && !is_entry {
                compiled_modules.push(CompiledModule {
                    module_id: module_id.clone(),
                    source_hash,
                    dep_hashes,
                });
            }
        }

        // Save Go stdlib cache if store has Go modules not already in cache
        if !cache_disabled {
            let all_go_modules: Vec<String> = checker
                .store
                .modules
                .keys()
                .filter(|id| id.starts_with("go:"))
                .cloned()
                .collect();
            let needs_save = !all_go_modules.is_empty()
                && go_cache.as_ref().is_none_or(|c| {
                    all_go_modules.len() != c.modules.len()
                        || all_go_modules.iter().any(|id| !c.modules.contains_key(id))
                });
            if needs_save {
                go_stdlib::save_go_stdlib_cache(checker.store, &all_go_modules);
            }
        }

        if !cache_disabled && !prelude_cache_hit {
            prelude_cache::save_prelude_cache(checker.store);
        }

        (
            checker.facts,
            checker.coercions,
            checker.resolutions,
            cached_modules,
            compiled_modules,
            checker.ufcs_methods,
        )
    };

    if !has_pre_check_errors {
        let pattern_ctx = pattern_analysis::Context::new(&store, &facts.or_pattern_error_spans);
        for module in store.modules.values() {
            for file in module.files.values() {
                for expression in &file.items {
                    pattern_analysis::check(expression, &pattern_ctx, &sink);
                }
            }
        }
        facts.pattern_issues = pattern_ctx.take_issues();
    }

    let errors = sink.take();

    let unused = if input.config.run_lints && !has_pre_check_errors {
        lint::lint_all_modules(&store, &facts, &sink)
    } else {
        UnusedInfo::default()
    };

    let mut mutations = MutationInfo::default();
    for (&binding_id, b) in facts.bindings.iter() {
        if b.mutated {
            mutations.mark_binding_mutated(binding_id);
        }
    }

    let lints = sink.take();

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

    for (mod_id, module) in store.modules {
        let is_internal = module.is_internal();

        definitions.extend(module.definitions);

        if is_internal {
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
        coercions,
        resolutions,
        cached_modules,
        ufcs_methods,
    };

    (result, facts)
}
