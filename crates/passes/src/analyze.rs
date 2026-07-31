use std::sync::Arc;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::LisetteDiagnostic;
use syntax::ast::BindingId;
use syntax::program::{EmitInput, MutationInfo, UnusedInfo};

use semantics::cache::{EmitStamp, save_module_cache};
use semantics::facts::{BindingFact, Usage};
use semantics::inference::AnalyzeInput;
use semantics::inference::{InferenceOutput, PARALLEL_THRESHOLD, run_inference};
use semantics::store::ENTRY_MODULE_ID;

use crate::passes;

pub struct Analysis {
    pub emit_input: EmitInput,
    pub emit_stamps: Vec<EmitStamp>,
    pub unreachable_modules: Vec<String>,
    bindings: HashMap<BindingId, BindingFact>,
    usages: HashSet<Usage>,
    diagnostics: Vec<LisetteDiagnostic>,
    entry_parse_status: semantics::inference::EntryParseStatus,
}

impl Analysis {
    pub fn diagnostics(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics
    }

    pub fn bindings(&self) -> &HashMap<BindingId, BindingFact> {
        &self.bindings
    }

    pub fn usages(&self) -> &HashSet<Usage> {
        &self.usages
    }

    pub fn errors(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics[..self.error_count()]
    }

    pub fn lints(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics[self.error_count()..]
    }

    pub fn push_error(&mut self, error: LisetteDiagnostic) {
        assert!(error.is_error());
        let index = self.error_count();
        self.diagnostics.insert(index, error);
    }

    pub fn take_diagnostics(&mut self) -> Vec<LisetteDiagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    fn error_count(&self) -> usize {
        self.diagnostics
            .partition_point(LisetteDiagnostic::is_error)
    }

    pub fn failed(&self) -> bool {
        !self.errors().is_empty()
    }

    pub fn has_parse_errors(&self) -> bool {
        self.entry_parse_status != semantics::inference::EntryParseStatus::Clean
    }

    pub fn entry_parse_failed(&self) -> bool {
        self.entry_parse_status == semantics::inference::EntryParseStatus::Failed
    }
}

pub fn analyze(input: AnalyzeInput) -> Analysis {
    let InferenceOutput {
        store,
        mut facts,
        sink,
        has_pre_check_errors,
        compiled_modules,
        cached_modules,
        cache_root,
        unreachable_modules,
        entry_parse_errors,
        entry_parse_status,
    } = run_inference(input);
    let run_lints = entry_parse_status == semantics::inference::EntryParseStatus::Clean;

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
    let bindings = std::mem::take(&mut facts.bindings);
    let usages = std::mem::take(&mut facts.usages);
    drop(facts);

    // Canonicalize diagnostic order so the output is stable regardless of
    // phase ordering, FxHashMap iteration, or parallel inference scheduling.
    let mut all_diagnostics = sink.into_diagnostics();
    all_diagnostics.sort_by(diagnostics::LisetteDiagnostic::sort_key);
    all_diagnostics.splice(0..0, entry_parse_errors.iter().cloned().map(Into::into));

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

    Analysis {
        emit_input: EmitInput {
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
        emit_stamps,
        unreachable_modules,
        bindings,
        usages,
        diagnostics: order_diagnostics_by_severity(all_diagnostics),
        entry_parse_status,
    }
}

fn order_diagnostics_by_severity(diagnostics: Vec<LisetteDiagnostic>) -> Vec<LisetteDiagnostic> {
    let (mut errors, lints): (Vec<_>, Vec<_>) = diagnostics
        .into_iter()
        .partition(LisetteDiagnostic::is_error);
    errors.extend(lints);
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    use semantics::inference::{AnalysisScope, CompilePhase, EntryFile, ProjectKind};
    use semantics::loader::MemoryLoader;

    #[test]
    fn analysis_classifies_diagnostics_by_severity() {
        let diagnostics = order_diagnostics_by_severity(vec![
            LisetteDiagnostic::warn("warning"),
            LisetteDiagnostic::error("error"),
            LisetteDiagnostic::info("info"),
        ]);

        let messages = diagnostics
            .iter()
            .map(LisetteDiagnostic::plain_message)
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["error", "warning", "info"]);
    }

    #[test]
    fn analysis_retains_navigation_links() {
        let source = "fn main() {\n  let value = 1\n  let _ = value\n}\n";
        let mut loader = MemoryLoader::new();
        loader.add_file(ENTRY_MODULE_ID, "main.lis", source);
        let locator = Default::default();

        let analysis = analyze(AnalyzeInput {
            load_siblings: false,
            scope: AnalysisScope::Standalone,
            loader: &loader,
            entry: Some(EntryFile::new(
                source.to_string(),
                "main.lis".to_string(),
                "main.lis".to_string(),
            )),
            compile_phase: CompilePhase::Check,
            project_kind: ProjectKind::Binary,
            locator: &locator,
            go_module: "",
            disable_cache: true,
        });

        let value_span = analysis
            .bindings()
            .values()
            .find(|binding| binding.name == "value")
            .map(|binding| binding.span)
            .expect("value binding should be retained");
        assert!(
            analysis
                .usages()
                .iter()
                .any(|usage| usage.definition_span == value_span),
            "value usage should link back to its retained binding"
        );
    }
}
