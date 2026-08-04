use std::sync::Arc;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::LisetteDiagnostic;
use syntax::ast::BindingId;
use syntax::program::{EmitInput, MutationInfo, UnusedInfo};

use semantics::AnalyzeInput;
use semantics::cache::{EmitStamp, save_package_cache};
use semantics::facts::{BindingFact, Usage};
use semantics::store::ENTRY_PACKAGE_ID;
use semantics::{InferenceOutput, PARALLEL_THRESHOLD, run_inference};

use crate::passes;

pub struct Analysis {
    pub emit_input: EmitInput,
    pub emit_stamps: Vec<EmitStamp>,
    pub unreachable_packages: Vec<String>,
    bindings: HashMap<BindingId, BindingFact>,
    usages: HashSet<Usage>,
    diagnostics: Vec<LisetteDiagnostic>,
    entry_parse_status: semantics::EntryParseStatus,
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
        self.entry_parse_status != semantics::EntryParseStatus::Clean
    }

    pub fn entry_parse_failed(&self) -> bool {
        self.entry_parse_status == semantics::EntryParseStatus::Failed
    }
}

pub fn analyze(input: AnalyzeInput) -> Analysis {
    let InferenceOutput {
        store,
        facts,
        sink,
        has_pre_check_errors,
        compiled_packages,
        cached_packages,
        cache_root,
        unreachable_packages,
        entry_parse,
    } = run_inference(input);
    let lint_mode = if entry_parse.is_clean() {
        passes::LintMode::Run
    } else {
        passes::LintMode::Skip
    };
    let entry_parse_status = entry_parse.status();
    let entry_parse_errors = entry_parse.into_errors();

    let unused = if has_pre_check_errors {
        UnusedInfo::default()
    } else {
        passes::run(&store, &facts, &sink, lint_mode)
    };
    let mut mutations = MutationInfo::default();
    for (&binding_id, b) in facts.bindings.iter() {
        if let Some(mutation) = b.mutation {
            mutations.record(binding_id, mutation);
        }
    }
    let bindings = facts.bindings;
    let usages = facts.usages;

    // Canonicalize diagnostic order so the output is stable regardless of
    // phase ordering, FxHashMap iteration, or parallel inference scheduling.
    let mut all_diagnostics = sink.into_diagnostics();
    all_diagnostics.sort_by(diagnostics::LisetteDiagnostic::sort_key);
    all_diagnostics.splice(0..0, entry_parse_errors.into_iter().map(Into::into));

    let emit_stamps: Vec<EmitStamp> = compiled_packages
        .iter()
        .map(|c| EmitStamp {
            package_id: c.package_id.clone(),
            artifact_hash: c.artifact_hash,
        })
        .collect();

    if let Some(ref project_root) = cache_root {
        let has_errors = all_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error());
        if !has_errors {
            let save = |compiled: &semantics::cache::CompiledPackage| {
                let file_ids: HashSet<u32> = store
                    .get_package(&compiled.package_id)
                    .map(|m| m.file_ids().collect())
                    .unwrap_or_default();

                let has_package_lints = all_diagnostics.iter().any(|diagnostic| {
                    !diagnostic.is_error()
                        && diagnostic
                            .file_id()
                            .map(|fid| file_ids.contains(&fid))
                            .unwrap_or(true)
                });
                if !has_package_lints
                    && let Err(e) = save_package_cache(compiled, &store, project_root)
                {
                    eprintln!(
                        "warning: failed to write cache for {}: {e}",
                        compiled.package_id
                    );
                }
            };
            if compiled_packages.len() < PARALLEL_THRESHOLD {
                compiled_packages.iter().for_each(save);
            } else {
                use rayon::prelude::*;
                compiled_packages.par_iter().for_each(save);
            }
        }
    }

    let mut files = HashMap::default();
    let mut definitions = HashMap::default();

    let go_package_ids: HashSet<String> = store
        .packages
        .keys()
        .filter(|id| id.starts_with(syntax::types::GO_IMPORT_PREFIX))
        .cloned()
        .collect();

    for (_, package) in store.packages {
        // Worker views are gone by now, so this unwraps without cloning.
        let package = Arc::try_unwrap(package).unwrap_or_else(|shared| (*shared).clone());
        let is_internal = package.is_internal();
        definitions.extend(package.definitions);

        // Internal typedef files remain available so the LSP can map their IDs
        // to URIs for go-to-definition. Source files identify their own package.
        if is_internal {
            files.extend(
                package
                    .files
                    .into_iter()
                    .filter(|(_, file)| file.is_d_lis()),
            );
            continue;
        }

        files.extend(package.files);
    }

    Analysis {
        emit_input: EmitInput {
            files,
            definitions,
            entry_package_id: ENTRY_PACKAGE_ID.to_string(),
            unused,
            mutations,
            cached_packages,
            equality_index: store.equality_index,
            test_index: store.test_index,
            go_package_names: store.go_package_names,
            go_package_ids,
        },
        emit_stamps,
        unreachable_packages,
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

    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, CompilePhase, EntryFile, ProjectKind};

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
        loader.add_file(ENTRY_PACKAGE_ID, "main.lis", source);
        let locator = Default::default();

        let analysis = analyze(AnalyzeInput {
            load_siblings: false,
            scope: AnalysisScope::Script {
                inside_project: false,
            },
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
