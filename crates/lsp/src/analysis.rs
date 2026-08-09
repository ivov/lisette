use std::path::Path;
use std::sync::{Arc, PoisonError};

use crate::protocol::*;
use rustc_hash::FxHashMap;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use passes::analyze;
use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile, ProjectKind, RecoverTarget};
use syntax::types::{CompoundKind, Type};

use crate::imports::PackageResolver;
use crate::loader::ProjectAnalysis;
use crate::paths::uri_to_package_file;
use crate::position::LineIndex;
use crate::snapshot::AnalysisSnapshot;
use crate::state::{AnalysisKey, SharedState, Workspace};

pub(crate) enum AnalysisError {
    Diagnostics(Vec<Diagnostic>),
    Superseded,
}

struct BuildInput {
    project: ProjectAnalysis,
    entry: Option<(String, String)>,
    uri: Option<Url>,
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
    pub(crate) fn key_for(&self, uri: &Url) -> Option<AnalysisKey> {
        let config = self.project.config_for(uri)?;
        let (package_id, filename, external_test) = uri_to_package_file(&config, uri)?;
        let filtered_from_package =
            !external_test && (filename.ends_with("_test.lis") || filename.ends_with(".d.lis"));
        if config.is_script() || filtered_from_package {
            return Some(AnalysisKey::Document { uri: uri.clone() });
        }
        Some(AnalysisKey::Package {
            external_test,
            package_id,
        })
    }

    pub(crate) fn validate(&self, uri: &Url) -> Option<LisetteDiagnostic> {
        let config = self.project.config_for(uri)?;
        let (package_id, filename, external_test) = uri_to_package_file(&config, uri)?;

        if let Some(dotted) = package_id
            .split('/')
            .find(|segment| segment.contains('.'))
            .filter(|_| dotted_directory_reaches_package_graph(uri, &filename, config.root()))
        {
            return Some(diagnostics::package_graph::dotted_package_directory(dotted));
        }

        if external_test {
            return semantics::loader::external_test_file_issue(&filename).map(
                |issue| match issue {
                    semantics::loader::ExternalTestFileIssue::WrongSuffix => {
                        diagnostics::package_graph::wrong_test_file_suffix(&filename)
                    }
                    semantics::loader::ExternalTestFileIssue::NotATestFile => {
                        diagnostics::package_graph::non_test_file_under_tests(&filename)
                    }
                },
            );
        }

        None
    }

    fn capture_build_input(&self, key: &AnalysisKey, workspace: &Workspace) -> Option<BuildInput> {
        let project = self.project.for_key(key)?;
        let (entry, uri) = match key {
            AnalysisKey::Document { uri } => {
                let config = self.project.config_for(uri)?;
                let (_, filename, _) = uri_to_package_file(&config, uri)?;
                let source = workspace.documents.get(uri)?.content().to_string();
                (Some((source, filename)), Some(uri.clone()))
            }
            AnalysisKey::Package { .. } => (None, None),
        };
        Some(BuildInput {
            project,
            entry,
            uri,
        })
    }

    fn run_analysis(
        &self,
        key: &AnalysisKey,
        input: BuildInput,
    ) -> Result<AnalysisSnapshot, Vec<Diagnostic>> {
        let BuildInput {
            project,
            entry,
            uri,
        } = input;
        let config = project.config;
        let loader = project.loader;
        let external_test = project.external_test;
        let entry_dir = project.entry_dir;

        let script = config.is_script();
        let (locator, manifest_error) = if script {
            (TypedefLocator::default(), None)
        } else {
            match TypedefLocator::from_project(config.root()) {
                Ok(r) => (r, None),
                Err(msg) => (TypedefLocator::default(), Some(msg)),
            }
        };

        let script_path = uri.as_ref().and_then(|uri| uri.to_file_path().ok());
        let script_setup = self
            .bindgen_setup
            .as_ref()
            .filter(|_| script && manifest_error.is_none())
            .zip(script_path.as_deref())
            .zip(entry.as_ref());
        let (locator, script_session, script_error) = match script_setup {
            Some(((setup, path), (source, _))) => match setup.for_script(source, path) {
                Ok((resolved, session)) => (resolved, session, None),
                Err(msg) => (locator, None, Some(msg)),
            },
            None => (locator, None, None),
        };

        // No bindgen runner on this copy: a keystroke must never shell out to Go.
        let packages = PackageResolver::new(locator.clone(), Arc::clone(&self.packages));

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
            entry: entry.map(|(source, filename)| {
                EntryFile::recovering(source, filename.clone(), filename)
            }),
            compile_phase: CompilePhase::Check,
            project_kind,
            locator: &locator,
            go_module: "",
            disable_cache: external_test,
            recover_target: recover_target(key),
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

        if let Some(msg) = script_error {
            analysis.push_error(
                LisetteDiagnostic::error(format!(
                    "Could not resolve this script's dependencies: {}",
                    msg
                ))
                .with_resolve_code("script_setup_failed"),
            );
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
        drop(script_session);

        Ok(AnalysisSnapshot::new(
            analysis,
            &config,
            &entry_dir,
            external_test,
            packages,
        ))
    }

    pub(crate) fn analysis_for(
        &self,
        key: &AnalysisKey,
    ) -> Result<Arc<AnalysisSnapshot>, AnalysisError> {
        if let Some(snapshot) = self.workspace().snapshot(key) {
            return Ok(snapshot);
        }

        let build = self
            .workspace()
            .build_lock(key)
            .ok_or(AnalysisError::Superseded)?;
        let _guard = build.lock().unwrap_or_else(PoisonError::into_inner);

        let (generation, input) = {
            let workspace = self.workspace();
            if !workspace
                .build_lock(key)
                .is_some_and(|current| Arc::ptr_eq(&current, &build))
            {
                return Err(AnalysisError::Superseded);
            }
            if let Some(snapshot) = workspace.snapshot(key) {
                return Ok(snapshot);
            }
            let input = self
                .capture_build_input(key, &workspace)
                .ok_or(AnalysisError::Superseded)?;
            (workspace.generation(), input)
        };

        let built = self.run_analysis(key, input);

        let mut workspace = self.workspace_mut();
        if workspace.generation() != generation {
            return Err(AnalysisError::Superseded);
        }
        let snapshot = Arc::new(built.map_err(AnalysisError::Diagnostics)?);
        if !workspace.install(key, generation, Arc::clone(&snapshot)) {
            return Err(AnalysisError::Superseded);
        }

        let recipients: Vec<Url> = workspace
            .documents
            .keys()
            .filter(|uri| self.key_for(uri).as_ref() == Some(key))
            .filter(|uri| self.validate(uri).is_none())
            .filter(|uri| snapshot.is_usable(uri))
            .cloned()
            .collect();
        for uri in recipients {
            if let Some(document) = workspace.documents.get_mut(&uri) {
                document.set_last_usable(Arc::clone(&snapshot));
            }
        }
        Ok(snapshot)
    }

    pub(crate) fn get_snapshot(&self, uri: &Url) -> Option<Arc<AnalysisSnapshot>> {
        let fallback = || self.workspace().documents.get(uri)?.last_usable();

        if self.validate(uri).is_some() {
            return fallback();
        }

        let key = self.key_for(uri)?;
        match self.analysis_for(&key) {
            Ok(snapshot) if snapshot.is_usable(uri) => Some(snapshot),
            _ => fallback(),
        }
    }

    pub(crate) fn diagnostics_for(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        if let Some(diagnostic) = self.validate(uri) {
            let source = self.workspace().documents.get(uri)?.content().to_string();
            return Some(vec![convert_diagnostic(
                &diagnostic,
                &LineIndex::new(&source),
            )]);
        }

        let key = self.key_for(uri)?;
        let snapshot = match self.analysis_for(&key) {
            Ok(snapshot) => snapshot,
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
                .map(|d| convert_diagnostic_in(d, document.line_index, Some(uri)))
                .collect(),
        )
    }

    pub(crate) fn open_document_snapshots(&self) -> Vec<Arc<AnalysisSnapshot>> {
        let uris: Vec<Url> = self.workspace().documents.keys().cloned().collect();
        let mut keys: Vec<AnalysisKey> = Vec::new();
        for uri in uris {
            if self.validate(&uri).is_some() {
                continue;
            }
            if let Some(key) = self.key_for(&uri)
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        keys.iter()
            .filter_map(|key| self.analysis_for(key).ok())
            .collect()
    }
}

fn recover_target(key: &AnalysisKey) -> RecoverTarget {
    match key {
        AnalysisKey::Package {
            external_test: true,
            package_id,
        } => RecoverTarget::Package(package_id.clone()),
        AnalysisKey::Package {
            external_test: false,
            ..
        } => RecoverTarget::Package(crate::paths::ENTRY_PACKAGE_ID.to_string()),
        AnalysisKey::Document { .. } => RecoverTarget::None,
    }
}

pub(crate) fn convert_diagnostic(d: &LisetteDiagnostic, index: &LineIndex) -> Diagnostic {
    convert_diagnostic_in(d, index, None)
}

fn related_information(
    d: &LisetteDiagnostic,
    index: &LineIndex,
    uri: &Url,
    anchor_file: Option<u32>,
) -> Option<Vec<serde_json::Value>> {
    let related: Vec<serde_json::Value> = d
        .secondary_labels()
        .filter(|(span, text)| !text.is_empty() && Some(span.file_id) == anchor_file)
        .map(|(span, text)| {
            serde_json::json!({
                "location": {
                    "uri": uri.to_string(),
                    "range": index.span_to_range(span),
                },
                "message": text,
            })
        })
        .collect();
    (!related.is_empty()).then_some(related)
}

pub(crate) fn convert_diagnostic_in(
    d: &LisetteDiagnostic,
    index: &LineIndex,
    uri: Option<&Url>,
) -> Diagnostic {
    let range = d
        .first_label_span()
        .map(|(offset, length)| index.offset_len_to_range(offset, length))
        .unwrap_or_default();
    let related = uri.and_then(|uri| related_information(d, index, uri, d.file_id()));

    Diagnostic {
        related_information: related,
        range,
        severity: Some(if d.is_error() {
            DiagnosticSeverity::ERROR
        } else if d.is_info() {
            DiagnosticSeverity::INFORMATION
        } else {
            DiagnosticSeverity::WARNING
        }),
        message: {
            let structured =
                d.label_count() >= 2 || d.plain_help().is_some_and(|help| help.contains('\n'));
            let mut msg = match d.plain_label() {
                Some(label) if !structured => label.to_string(),
                _ => d.plain_message().to_string(),
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
    fn multi_label_diagnostic_leads_with_the_message_and_reports_related_locations() {
        let index = LineIndex::new("fn write() {}\nfn call() {}\n");
        let uri = Url::parse("file:///x.lis").unwrap();
        let diagnostic = convert_diagnostic_in(
            &LisetteDiagnostic::error("`File` does not implement `Writer`")
                .with_span_label(&syntax::ast::Span::new(0, 17, 4), "`Writer` needed here")
                .with_span_label(
                    &syntax::ast::Span::new(0, 3, 5),
                    "`Writer` requires `fn () -> int`",
                ),
            &index,
            Some(&uri),
        );

        assert_eq!(diagnostic.message, "`File` does not implement `Writer`");
        let related = diagnostic
            .related_information
            .expect("a second label should become related information");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0]["message"], "`Writer` requires `fn () -> int`");
        assert_eq!(related[0]["location"]["uri"], "file:///x.lis");
    }

    #[test]
    fn multi_line_help_leads_with_the_message() {
        let span = syntax::ast::Span::new(0, 0, 1);
        let diagnostic = convert_diagnostic(
            &LisetteDiagnostic::error("`File` does not implement `ReadWriter`")
                .with_span_label(&span, "`ReadWriter` needed here")
                .with_help("Two methods do not satisfy `ReadWriter`:\nMissing:\n  fn read(self: File) -> string"),
            &LineIndex::new("x"),
        );
        assert!(
            diagnostic
                .message
                .starts_with("`File` does not implement `ReadWriter` · Two methods"),
            "got: {}",
            diagnostic.message
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
