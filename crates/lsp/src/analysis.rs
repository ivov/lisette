use std::sync::Arc;

use miette::Diagnostic as MietteDiagnostic;
use rustc_hash::FxHashMap;
use tower_lsp::lsp_types::*;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use passes::analyze;
use semantics::inference::{AnalyzeInput, CompilePhase, EntryFile, ProjectKind, SemanticConfig};
use syntax::desugar;
use syntax::lex::Lexer;
use syntax::parse::Parser;
use syntax::types::{CompoundKind, Type};

use crate::position::LineIndex;
use crate::snapshot::AnalysisSnapshot;
use crate::state::{AnalysisRequest, SharedState};

enum AnalysisError {
    Diagnostics(Vec<Diagnostic>),
    Superseded,
}

/// Extract the constructor type name, unwrapping `Ref<T>` and peeling aliases.
pub(crate) fn type_name(ty: &Type, snapshot: &AnalysisSnapshot) -> Option<String> {
    let resolved = syntax::types::peel_alias(ty, |id| snapshot.definitions().get(id));
    match &resolved {
        Type::Nominal { id, .. } => Some(id.to_string()),
        Type::Compound {
            kind: CompoundKind::Ref,
            args,
        } => args.first().and_then(|ty| type_name(ty, snapshot)),
        Type::Compound { kind, .. } => Some(format!("prelude.{}", kind.leaf_name())),
        Type::Simple(kind) => Some(format!("prelude.{}", kind.leaf_name())),
        Type::Array { .. } => Some("prelude.Array".to_string()),
        _ => None,
    }
}

pub(crate) fn offset_in_span(offset: u32, span: &syntax::ast::Span) -> bool {
    offset >= span.byte_offset && offset < span.byte_offset + span.byte_length
}

/// Look up the module name for an import alias in a file.
pub(crate) fn find_module_by_alias(
    file: &syntax::program::File,
    alias: &str,
    go_package_names: &FxHashMap<String, String>,
) -> Option<String> {
    file.imports().into_iter().find_map(|import| {
        if import.effective_alias(go_package_names).as_deref() == Some(alias) {
            Some(import.name.to_string())
        } else {
            None
        }
    })
}

impl SharedState {
    async fn run_analysis(&self, uri: &Url) -> Result<AnalysisSnapshot, Vec<Diagnostic>> {
        let project = self.project.for_analysis(uri).await.ok_or_else(Vec::new)?;
        let config = project.config;
        let filename = project.filename;
        let loader = project.loader;

        let source = self
            .documents
            .get(uri)
            .map(|document| document.content().to_string())
            .ok_or_else(Vec::new)?;

        let lex_result = Lexer::new(&source, 0).lex();
        if lex_result.failed() {
            let line_index = LineIndex::new(&source);
            return Err(lex_result
                .errors
                .into_iter()
                .map(|e| {
                    let diag: LisetteDiagnostic = e.into();
                    convert_diagnostic(&diag, &line_index)
                })
                .collect());
        }

        let parse_result = Parser::new(lex_result.tokens, &source).parse();
        let desugar_result = desugar::desugar(parse_result.ast);

        let has_parse_errors = !parse_result.errors.is_empty() || !desugar_result.errors.is_empty();
        let parse_errors: Vec<LisetteDiagnostic> = parse_result
            .errors
            .into_iter()
            .chain(desugar_result.errors)
            .map(Into::into)
            .collect();

        let standalone = config.is_standalone();
        let (locator, manifest_error) = if standalone {
            (TypedefLocator::default(), None)
        } else {
            match TypedefLocator::from_project(config.root()) {
                Ok(r) => (r, None),
                Err(msg) => (TypedefLocator::default(), Some(msg)),
            }
        };

        let (locator, session, bindgen_error) = if standalone || manifest_error.is_some() {
            (locator, None, None)
        } else if let Some(setup) = self.bindgen_setup.as_ref() {
            match setup.for_project(config.root(), locator.target()) {
                Ok(session) => {
                    let with_runner = locator.clone().with_bindgen(session.bindgen.clone());
                    (with_runner, Some(session), None)
                }
                Err(msg) => (locator, None, Some(msg)),
            }
        } else {
            (locator, None, None)
        };

        let project_kind = if standalone || config.root().join("src/main.lis").exists() {
            ProjectKind::Binary
        } else {
            ProjectKind::Library
        };

        let analyze_output = analyze(AnalyzeInput {
            config: SemanticConfig {
                run_lints: !has_parse_errors,
                standalone_mode: standalone,
                load_siblings: true,
            },
            loader: &loader,
            entry: Some(EntryFile {
                source,
                display_path: filename.clone(),
                filename,
                ast: desugar_result.ast,
                file_comment: parse_result.file_comment,
            }),
            project_root: if standalone {
                None
            } else {
                Some(config.root().to_path_buf())
            },
            compile_phase: CompilePhase::Check,
            project_kind,
            emit_tests: false,
            locator,
            go_module: String::new(),
            disable_cache: false,
        });
        let mut result = analyze_output.result;
        let facts = analyze_output.facts;

        if has_parse_errors {
            let mut all_errors = parse_errors;
            all_errors.append(&mut result.errors);
            result.errors = all_errors;
        }

        if let Some(msg) = manifest_error {
            result
                .errors
                .push(LisetteDiagnostic::error(msg).with_resolve_code("manifest_error"));
        }

        if let Some(msg) = bindgen_error {
            result.errors.push(
                LisetteDiagnostic::error(format!(
                    "Could not start bindgen for this project: {}",
                    msg
                ))
                .with_resolve_code("bindgen_setup_failed"),
            );
        }

        // Release the target lock before the lock-free snapshot construction.
        drop(session);

        Ok(AnalysisSnapshot::new(
            result,
            facts,
            has_parse_errors,
            &config,
            uri,
        ))
    }

    async fn run_analysis_cached(&self, uri: &Url) -> Result<Arc<AnalysisSnapshot>, AnalysisError> {
        let document = self.documents.get(uri).ok_or(AnalysisError::Superseded)?;
        let request = document.request_analysis();
        drop(document);

        let revision = match request {
            AnalysisRequest::Cached(snapshot) => return Ok(snapshot),
            AnalysisRequest::Required(revision) => revision,
        };

        let snapshot = Arc::new(
            self.run_analysis(uri)
                .await
                .map_err(AnalysisError::Diagnostics)?,
        );

        if let Some(mut document) = self.documents.get_mut(uri)
            && document.cache_analysis(&revision, Arc::clone(&snapshot))
        {
            return Ok(snapshot);
        }

        Err(AnalysisError::Superseded)
    }

    pub(crate) async fn analyze_and_convert(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        let snapshot = match self.run_analysis_cached(uri).await {
            Ok(s) => s,
            Err(AnalysisError::Diagnostics(diagnostics)) => return Some(diagnostics),
            Err(AnalysisError::Superseded) => return None,
        };

        let Some(document) = snapshot.document(uri) else {
            return Some(vec![]);
        };

        Some(
            snapshot
                .result
                .errors
                .iter()
                .chain(&snapshot.result.lints)
                .filter(|d| {
                    let fid = d.file_id();
                    fid == Some(document.file_id) || fid.is_none()
                })
                .map(|d| convert_diagnostic(d, document.line_index))
                .collect(),
        )
    }

    pub(crate) async fn get_snapshot(&self, uri: &Url) -> Option<Arc<AnalysisSnapshot>> {
        self.run_analysis_cached(uri).await.ok().or_else(|| {
            self.documents
                .get(uri)
                .and_then(|document| document.last_valid_analysis())
        })
    }

    pub(crate) fn open_document_snapshots(&self) -> Vec<Arc<AnalysisSnapshot>> {
        self.documents
            .iter()
            .filter_map(|document| match document.request_analysis() {
                AnalysisRequest::Cached(snapshot) => Some(snapshot),
                AnalysisRequest::Required(_) => None,
            })
            .collect()
    }
}

pub(crate) fn convert_diagnostic(d: &LisetteDiagnostic, index: &LineIndex) -> Diagnostic {
    let range = d
        .labels()
        .and_then(|labels| labels.into_iter().next())
        .map(|label| index.offset_len_to_range(label.offset(), label.len()))
        .unwrap_or_default();

    Diagnostic {
        range,
        severity: Some(if d.is_error() {
            DiagnosticSeverity::ERROR
        } else if d.is_info() {
            DiagnosticSeverity::INFORMATION
        } else {
            DiagnosticSeverity::WARNING
        }),
        message: {
            let mut msg = match d.plain_label() {
                Some(label) => label.to_string(),
                None => d.plain_message().to_string(),
            };
            if let Some(help) = d.plain_help() {
                msg.push_str(" · ");
                msg.push_str(help);
            }
            if let Some(note) = d.plain_note() {
                msg.push_str(" · ");
                msg.push_str(note);
            }
            msg
        },
        source: Some("lisette".into()),
        code: d.code_str().map(|s| NumberOrString::String(s.to_string())),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_maps_to_lsp_information() {
        let index = LineIndex::new("");
        let diagnostic = convert_diagnostic(&LisetteDiagnostic::info("advisory"), &index);
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn warning_maps_to_lsp_warning() {
        let index = LineIndex::new("");
        let diagnostic = convert_diagnostic(&LisetteDiagnostic::warn("w"), &index);
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
    }
}
