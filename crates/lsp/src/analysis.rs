use std::path::Path;
use std::sync::Arc;

use crate::protocol::*;
use rustc_hash::FxHashMap;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use passes::analyze;
use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile, ProjectKind};
use syntax::types::{CompoundKind, Type};

use crate::position::LineIndex;
use crate::snapshot::AnalysisSnapshot;
use crate::state::{AnalysisRequest, DocumentState, SharedState};

enum AnalysisError {
    Diagnostics(Vec<Diagnostic>),
    Superseded,
}

fn dotted_directory_reaches_package_graph(uri: &Url, filename: &str, root: &Path) -> bool {
    if !semantics::loader::is_typedef_file(filename) {
        return true;
    }

    let Ok(file) = uri.to_file_path() else {
        return true;
    };

    let mut outermost = None;
    let mut walk = file.parent();
    while let Some(dir) = walk.filter(|dir| *dir != root && dir.starts_with(root)) {
        if dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains('.'))
        {
            outermost = Some(dir);
        }
        walk = dir.parent();
    }

    outermost.is_none_or(tree_has_compiled_source)
}

fn tree_has_compiled_source(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true;
    };
    entries.flatten().any(|entry| {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            return tree_has_compiled_source(&entry.path());
        }
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(".lis") && !semantics::loader::is_typedef_file(name))
    })
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

/// Look up the package name for an import alias in a file.
pub(crate) fn find_package_by_alias(
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
    fn run_analysis(&self, uri: &Url) -> Result<AnalysisSnapshot, Vec<Diagnostic>> {
        let project = self.project.for_analysis(uri).ok_or_else(Vec::new)?;
        let config = project.config;
        let filename = project.filename;
        let loader = project.loader;
        let external_test = project.external_test;

        let source = self
            .documents()
            .get(uri)
            .map(|document| document.content().to_string())
            .ok_or_else(Vec::new)?;

        if let Some(dotted) = project
            .package_id
            .split('/')
            .find(|segment| segment.contains('.'))
            .filter(|_| dotted_directory_reaches_package_graph(uri, &filename, config.root()))
        {
            return Err(vec![convert_diagnostic(
                &diagnostics::package_graph::dotted_package_directory(dotted),
                &LineIndex::new(&source),
            )]);
        }

        if external_test && let Some(issue) = semantics::loader::external_test_file_issue(&filename)
        {
            let diagnostic = match issue {
                semantics::loader::ExternalTestFileIssue::WrongSuffix => {
                    diagnostics::package_graph::wrong_test_file_suffix(&filename)
                }
                semantics::loader::ExternalTestFileIssue::NotATestFile => {
                    diagnostics::package_graph::non_test_file_under_tests(&filename)
                }
            };
            return Err(vec![convert_diagnostic(
                &diagnostic,
                &LineIndex::new(&source),
            )]);
        }

        let script = config.is_script();
        let (locator, manifest_error) = if script {
            (TypedefLocator::default(), None)
        } else {
            match TypedefLocator::from_project(config.root()) {
                Ok(r) => (r, None),
                Err(msg) => (TypedefLocator::default(), Some(msg)),
            }
        };

        let (locator, session, bindgen_error) = if script || manifest_error.is_some() {
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

        let project_kind = if script || config.root().join("src/main.lis").exists() {
            ProjectKind::Binary
        } else {
            ProjectKind::Library
        };

        let mut analysis = analyze(AnalyzeInput {
            load_siblings: true,
            scope: if script {
                AnalysisScope::Script {
                    inside_project: false,
                }
            } else {
                AnalysisScope::Project(config.root().to_path_buf())
            },
            loader: &loader,
            entry: if external_test {
                None
            } else {
                Some(EntryFile::recovering(source, filename.clone(), filename))
            },
            compile_phase: CompilePhase::Check,
            project_kind,
            locator: &locator,
            go_module: "",
            disable_cache: external_test,
        });
        let entry_parse_failed = analysis.entry_parse_failed();

        if entry_parse_failed {
            let line_index = LineIndex::new(
                analysis
                    .emit_input
                    .files
                    .get(&0)
                    .map(|file| file.source.as_str())
                    .unwrap_or_default(),
            );
            return Err(analysis
                .errors()
                .iter()
                .map(|diagnostic| convert_diagnostic(diagnostic, &line_index))
                .collect());
        }

        if let Some(msg) = manifest_error {
            analysis.push_error(LisetteDiagnostic::error(msg).with_resolve_code("manifest_error"));
        }

        if let Some(msg) = bindgen_error {
            analysis.push_error(
                LisetteDiagnostic::error(format!(
                    "Could not start bindgen for this project: {}",
                    msg
                ))
                .with_resolve_code("bindgen_setup_failed"),
            );
        }

        // Release the target lock before the lock-free snapshot construction.
        drop(session);

        Ok(AnalysisSnapshot::new(analysis, &config, uri, external_test))
    }

    fn run_analysis_cached(&self, uri: &Url) -> Result<Arc<AnalysisSnapshot>, AnalysisError> {
        let documents = self.documents();
        let document = documents.get(uri).ok_or(AnalysisError::Superseded)?;
        let request = document.request_analysis();
        drop(documents);

        let revision = match request {
            AnalysisRequest::Cached(snapshot) => return Ok(snapshot),
            AnalysisRequest::Required(revision) => revision,
        };

        let snapshot = Arc::new(self.run_analysis(uri).map_err(AnalysisError::Diagnostics)?);

        if let Some(document) = self.documents_mut().get_mut(uri)
            && document.cache_analysis(&revision, Arc::clone(&snapshot))
        {
            return Ok(snapshot);
        }

        Err(AnalysisError::Superseded)
    }

    pub(crate) fn analyze_and_convert(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        let snapshot = match self.run_analysis_cached(uri) {
            Ok(s) => s,
            Err(AnalysisError::Diagnostics(diagnostics)) => return Some(diagnostics),
            Err(AnalysisError::Superseded) => return None,
        };

        let Some(document) = snapshot.document(uri) else {
            return Some(vec![]);
        };

        Some(
            snapshot
                .analysis
                .diagnostics()
                .iter()
                .filter(|d| {
                    let fid = d.file_id();
                    fid == Some(document.file_id) || fid.is_none()
                })
                .map(|d| convert_diagnostic(d, document.line_index))
                .collect(),
        )
    }

    pub(crate) fn get_snapshot(&self, uri: &Url) -> Option<Arc<AnalysisSnapshot>> {
        match self.run_analysis_cached(uri) {
            Ok(snapshot) => Some(snapshot),
            Err(_) => self
                .documents()
                .get(uri)
                .and_then(DocumentState::last_valid_analysis),
        }
    }

    pub(crate) fn open_document_snapshots(&self) -> Vec<Arc<AnalysisSnapshot>> {
        self.documents()
            .values()
            .filter_map(|document| match document.request_analysis() {
                AnalysisRequest::Cached(snapshot) => Some(snapshot),
                AnalysisRequest::Required(_) => None,
            })
            .collect()
    }
}

pub(crate) fn convert_diagnostic(d: &LisetteDiagnostic, index: &LineIndex) -> Diagnostic {
    let range = d
        .first_label_span()
        .map(|(offset, length)| index.offset_len_to_range(offset, length))
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
            if let Some(first) = msg.chars().next()
                && first.is_ascii_lowercase()
            {
                msg.replace_range(0..1, &first.to_ascii_uppercase().to_string());
            }
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

    #[test]
    fn prose_label_is_capitalized() {
        let span = syntax::ast::Span::new(0, 0, 1);
        let diagnostic = convert_diagnostic(
            &LisetteDiagnostic::warn("Unused function")
                .with_span_label(&span, "never called")
                .with_help("Call or remove this function"),
            &LineIndex::new("x"),
        );
        assert_eq!(
            diagnostic.message,
            "Never called · Call or remove this function"
        );
    }

    #[test]
    fn identifier_led_label_is_left_untouched() {
        let span = syntax::ast::Span::new(0, 0, 1);
        let diagnostic = convert_diagnostic(
            &LisetteDiagnostic::error("Name not found")
                .with_span_label(&span, "`missing` not found in package `root`"),
            &LineIndex::new("x"),
        );
        assert_eq!(diagnostic.message, "`missing` not found in package `root`");
    }
}
