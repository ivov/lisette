use std::sync::Arc;

use miette::Diagnostic as MietteDiagnostic;
use rustc_hash::FxHashMap;
use tower_lsp::lsp_types::*;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use semantics::analyze::{AnalyzeInput, CompilePhase, SemanticConfig, analyze};
use syntax::desugar;
use syntax::lex::Lexer;
use syntax::parse::Parser;

use crate::paths::{module_id_to_dir, uri_to_module_file};
use crate::position::LineIndex;
use crate::snapshot::AnalysisSnapshot;
use crate::state::{CachedSnapshot, SharedState};

/// Extract the constructor type name, unwrapping `Ref<T>` and peeling aliases.
pub(crate) fn type_name(ty: &syntax::types::Type) -> Option<String> {
    match ty {
        syntax::types::Type::Nominal {
            underlying_ty: Some(u),
            ..
        } => type_name(u),
        syntax::types::Type::Nominal { id, .. } => Some(id.to_string()),
        syntax::types::Type::Compound {
            kind: syntax::types::CompoundKind::Ref,
            args,
        } => args.first().and_then(type_name),
        syntax::types::Type::Compound { kind, .. } => Some(format!("prelude.{}", kind.leaf_name())),
        syntax::types::Type::Simple(kind) => Some(format!("prelude.{}", kind.leaf_name())),
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
    pub(crate) async fn run_analysis(
        &self,
        uri: &Url,
    ) -> Result<AnalysisSnapshot, Vec<Diagnostic>> {
        let config = self.ensure_config(uri).await.ok_or_else(Vec::new)?;
        let (module_id, filename) = uri_to_module_file(&config, uri).ok_or_else(Vec::new)?;

        let source = self
            .documents
            .get(uri)
            .map(|doc| doc.content.clone())
            .ok_or_else(Vec::new)?;

        let loader_clone = {
            let mut loader = self.loader.write().await;
            let module_dir = module_id_to_dir(&config, &module_id);
            loader.set_entry_module_path(Some(module_dir));
            let clone = loader.clone();
            loader.set_entry_module_path(None);
            clone
        };

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

        let (locator, manifest_error) = if config.standalone_mode {
            (TypedefLocator::default(), None)
        } else {
            match TypedefLocator::from_project(&config.root) {
                Ok(r) => (r, None),
                Err(msg) => (TypedefLocator::default(), Some(msg)),
            }
        };

        let (mut result, facts) = analyze(AnalyzeInput {
            config: SemanticConfig {
                run_lints: !has_parse_errors,
                standalone_mode: config.standalone_mode,
                load_siblings: true,
            },
            loader: &loader_clone,
            source,
            filename,
            ast: desugar_result.ast,
            project_root: if config.standalone_mode {
                None
            } else {
                Some(config.root.clone())
            },
            compile_phase: CompilePhase::Check,
            locator,
        });

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

        Ok(AnalysisSnapshot::new(
            result,
            facts,
            has_parse_errors,
            &config,
            uri,
        ))
    }

    pub(crate) async fn run_analysis_cached(
        &self,
        uri: &Url,
    ) -> Result<Arc<AnalysisSnapshot>, Vec<Diagnostic>> {
        let pre_version = self.documents.get(uri).map(|d| d.version);

        if let Some(cached) = self.snapshots.get(uri)
            && Some(cached.version) == pre_version
        {
            return Ok(Arc::clone(&cached.snapshot));
        }

        let snapshot = Arc::new(self.run_analysis(uri).await?);

        let post_version = self.documents.get(uri).map(|d| d.version);
        if pre_version == post_version
            && let Some(version) = pre_version
        {
            if !snapshot.has_parse_errors {
                self.last_valid_snapshot
                    .insert(uri.clone(), Arc::clone(&snapshot));
            }
            self.snapshots.insert(
                uri.clone(),
                CachedSnapshot {
                    snapshot: Arc::clone(&snapshot),
                    version,
                },
            );
        }

        Ok(snapshot)
    }

    pub(crate) async fn analyze_and_convert(&self, uri: &Url) -> Vec<Diagnostic> {
        let snapshot = match self.run_analysis_cached(uri).await {
            Ok(s) => s,
            Err(syntax_diagnostics) => return syntax_diagnostics,
        };

        let Some(file_id) = snapshot.get_file_id(uri) else {
            return vec![];
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return vec![];
        };

        snapshot
            .result
            .errors
            .iter()
            .chain(&snapshot.result.lints)
            .filter(|d| {
                let fid = d.file_id();
                fid == Some(file_id) || fid.is_none()
            })
            .map(|d| convert_diagnostic(d, line_index))
            .collect()
    }

    pub(crate) async fn get_snapshot(&self, uri: &Url) -> Option<Arc<AnalysisSnapshot>> {
        self.run_analysis_cached(uri)
            .await
            .ok()
            .or_else(|| self.last_valid_snapshot.get(uri).map(|s| Arc::clone(&s)))
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
        } else {
            DiagnosticSeverity::WARNING
        }),
        message: {
            let mut msg = d.plain_message().to_string();
            if let Some(help) = d.plain_help() {
                msg.push_str(". ");
                msg.push_str(help);
            }
            if let Some(note) = d.plain_note() {
                msg.push_str(". ");
                msg.push_str(note);
            }
            msg
        },
        source: Some("lisette".into()),
        code: d.code_str().map(|s| NumberOrString::String(s.to_string())),
        ..Default::default()
    }
}
