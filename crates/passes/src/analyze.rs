use std::sync::Arc;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::SemanticResult;
use syntax::program::{MutationInfo, UnusedInfo};

use semantics::cache::{EmitStamp, compute_emit_artifact_hash, save_module_cache};
use semantics::facts::Facts;
use semantics::inference::AnalyzeInput;
use semantics::inference::{InferenceOutput, PARALLEL_THRESHOLD, run_inference};
use semantics::store::ENTRY_MODULE_ID;

use crate::passes;

/// Wraps `SemanticResult` plus per-module emit stamps the CLI uses to update
/// the cache after a successful artifact write.
pub struct AnalyzeOutput {
    pub result: SemanticResult,
    pub facts: Facts,
    pub emit_stamps: Vec<EmitStamp>,
    pub unreachable_modules: Vec<String>,
}

pub fn analyze(input: AnalyzeInput) -> AnalyzeOutput {
    let run_lints = input.config.run_lints;
    let go_module = input.go_module.clone();
    let project_root = input.project_root.clone();

    let InferenceOutput {
        mut store,
        mut facts,
        ufcs_methods,
        sink,
        has_pre_check_errors,
        compiled_modules,
        cached_modules,
        cache_enabled,
        unreachable_modules,
    } = run_inference(input);

    store.build_closed_domains();

    let analysis = semantics::context::AnalysisContext::new(&store, &ufcs_methods);

    let mut unused = UnusedInfo::default();
    if !has_pre_check_errors {
        passes::run(&analysis, &mut facts, &sink, &mut unused, run_lints);
    }
    let mut mutations = MutationInfo::default();
    for (&binding_id, b) in facts.bindings.iter() {
        if let Some(mutation) = b.mutation {
            mutations.record(binding_id, mutation);
        }
    }

    // Canonicalize diagnostic order so the output is stable regardless of
    // phase ordering, FxHashMap iteration, or parallel inference scheduling.
    let mut all_diagnostics = sink.take();
    all_diagnostics.sort_by(diagnostics::LisetteDiagnostic::sort_key);
    let (errors, lints): (Vec<_>, Vec<_>) = all_diagnostics.into_iter().partition(|d| d.is_error());

    let emit_stamps: Vec<EmitStamp> = compiled_modules
        .iter()
        .map(|c| EmitStamp {
            module_id: c.module_id.clone(),
            artifact_hash: compute_emit_artifact_hash(c.production_hash, &go_module),
        })
        .collect();

    if cache_enabled && let Some(ref project_root) = project_root {
        let has_errors = errors.iter().any(|e| e.is_error());
        if !has_errors {
            let save = |compiled: &semantics::cache::CompiledModule| {
                let file_ids: HashSet<u32> = store
                    .get_module(&compiled.module_id)
                    .map(|m| m.file_ids().collect())
                    .unwrap_or_default();

                let has_module_lints = lints.iter().any(|lint| {
                    lint.file_id()
                        .map(|fid| file_ids.contains(&fid))
                        .unwrap_or(true)
                });
                if !has_module_lints
                    && let Err(e) = save_module_cache(compiled, &store, project_root, &ufcs_methods)
                {
                    eprintln!(
                        "warning: failed to write cache for {}: {e}",
                        compiled.module_id
                    );
                }
            };
            if compiled_modules.len() < PARALLEL_THRESHOLD {
                compiled_modules.iter().for_each(save);
            } else {
                use rayon::prelude::*;
                compiled_modules.par_iter().for_each(save);
            }
        }
    }

    let mut files = HashMap::default();
    let mut definitions = HashMap::default();

    let go_module_ids: HashSet<String> = store
        .modules
        .keys()
        .filter(|id| id.starts_with(syntax::types::GO_IMPORT_PREFIX))
        .cloned()
        .collect();

    for (_, module) in store.modules {
        // Worker views are gone by now, so this unwraps without cloning.
        let module = Arc::try_unwrap(module).unwrap_or_else(|shared| (*shared).clone());
        let is_internal = module.is_internal();
        definitions.extend(module.definitions);

        // Internal typedef files remain available so the LSP can map their IDs
        // to URIs for go-to-definition. Source files identify their own module.
        if is_internal {
            files.extend(module.files.into_iter().filter(|(_, file)| file.is_d_lis()));
            continue;
        }

        files.extend(module.files);
    }

    let result = SemanticResult {
        files,
        definitions,
        errors,
        lints,
        entry_module_id: ENTRY_MODULE_ID.to_string(),
        unused,
        mutations,
        cached_modules,
        ufcs_methods,
        equality_index: store.equality_index,
        test_index: store.test_index,
        typedef_paths: store.typedef_paths,
        go_package_names: store.go_package_names,
        go_module_ids,
    };

    AnalyzeOutput {
        result,
        facts,
        emit_stamps,
        unreachable_modules,
    }
}
