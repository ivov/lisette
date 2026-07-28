use std::sync::Arc;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::SemanticResult;
use syntax::program::{EmitInput, MutationInfo, UnusedInfo};

use semantics::cache::{EmitStamp, save_module_cache};
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

    let InferenceOutput {
        store,
        mut facts,
        sink,
        has_pre_check_errors,
        compiled_modules,
        cached_modules,
        cache_root,
        unreachable_modules,
    } = run_inference(input);

    let mut unused = UnusedInfo::default();
    if !has_pre_check_errors {
        passes::run(&store, &mut facts, &sink, &mut unused, run_lints);
    }
    let mut mutations = MutationInfo::default();
    for (&binding_id, b) in facts.bindings.iter() {
        if let Some(mutation) = b.mutation {
            mutations.record(binding_id, mutation);
        }
    }

    // Canonicalize diagnostic order so the output is stable regardless of
    // phase ordering, FxHashMap iteration, or parallel inference scheduling.
    let mut all_diagnostics = sink.into_diagnostics();
    all_diagnostics.sort_by(diagnostics::LisetteDiagnostic::sort_key);

    let emit_stamps: Vec<EmitStamp> = compiled_modules
        .iter()
        .map(|c| EmitStamp {
            module_id: c.module_id.clone(),
            artifact_hash: c.artifact_hash,
        })
        .collect();

    if let Some(ref project_root) = cache_root {
        let has_errors = all_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error());
        if !has_errors {
            let save = |compiled: &semantics::cache::CompiledModule| {
                let file_ids: HashSet<u32> = store
                    .get_module(&compiled.module_id)
                    .map(|m| m.file_ids().collect())
                    .unwrap_or_default();

                let has_module_lints = all_diagnostics.iter().any(|diagnostic| {
                    !diagnostic.is_error()
                        && diagnostic
                            .file_id()
                            .map(|fid| file_ids.contains(&fid))
                            .unwrap_or(true)
                });
                if !has_module_lints
                    && let Err(e) = save_module_cache(compiled, &store, project_root)
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

    let result = SemanticResult::new(
        EmitInput {
            files,
            definitions,
            entry_module_id: ENTRY_MODULE_ID.to_string(),
            unused,
            mutations,
            cached_modules,
            equality_index: store.equality_index,
            test_index: store.test_index,
            go_package_names: store.go_package_names,
            go_module_ids,
        },
        all_diagnostics,
    );

    AnalyzeOutput {
        result,
        facts,
        emit_stamps,
        unreachable_modules,
    }
}
