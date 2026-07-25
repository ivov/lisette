use rustc_hash::FxHashMap as HashMap;

use diagnostics::SemanticResult;
use semantics::facts::Facts;
use syntax::program::{Definition, File};
use syntax::types::Symbol;
use tower_lsp::lsp_types::{Position, Url};

use crate::paths::{ENTRY_MODULE_ID, module_file_to_path};
use crate::position::LineIndex;
use crate::project::ProjectConfig;

pub(crate) struct AnalysisSnapshot {
    pub(crate) result: SemanticResult,
    facts: Facts,
    pub(crate) has_parse_errors: bool,
    sources: HashMap<u32, SnapshotSource>,
}

pub(crate) struct SnapshotSource {
    pub(crate) uri: Url,
    pub(crate) line_index: LineIndex,
}

pub(crate) struct SnapshotDocument<'a> {
    pub(crate) file_id: u32,
    pub(crate) file: &'a File,
    pub(crate) line_index: &'a LineIndex,
}

pub(crate) struct SnapshotPosition<'a> {
    pub(crate) document: SnapshotDocument<'a>,
    pub(crate) offset: u32,
}

impl AnalysisSnapshot {
    pub(crate) fn new(
        result: SemanticResult,
        facts: Facts,
        has_parse_errors: bool,
        config: &ProjectConfig,
        analyzed_uri: &Url,
    ) -> Self {
        let mut sources = HashMap::default();

        let analyzed_path = analyzed_uri.to_file_path().ok();
        let analyzed_filename = analyzed_path
            .as_ref()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()));
        let analyzed_dir = analyzed_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        for (file_id, file) in &result.files {
            let uri = if file.module_id == ENTRY_MODULE_ID {
                if analyzed_filename.as_deref() == Some(&file.name) {
                    analyzed_uri.clone()
                } else if let Some(ref dir) = analyzed_dir {
                    let sibling_path = dir.join(&file.name);
                    match Url::from_file_path(&sibling_path) {
                        Ok(uri) => uri,
                        Err(_) => continue,
                    }
                } else {
                    continue;
                }
            } else if let Some(typedef_path) = result.typedef_paths.get(file_id) {
                // The synthetic `file.name` for go: typedefs does not match the
                // on-disk filename — use the path the locator captured.
                match Url::from_file_path(typedef_path) {
                    Ok(uri) => uri,
                    Err(_) => continue,
                }
            } else if file.module_id.starts_with("go:") || file.module_id == "prelude" {
                // Embedded typedef with no recorded on-disk path; nothing to navigate to.
                continue;
            } else {
                let path = module_file_to_path(config, &file.module_id, &file.name);
                match Url::from_file_path(&path) {
                    Ok(uri) => uri,
                    Err(_) => continue,
                }
            };

            sources.insert(
                *file_id,
                SnapshotSource {
                    uri,
                    line_index: LineIndex::new(&file.source),
                },
            );
        }

        Self {
            result,
            facts,
            has_parse_errors,
            sources,
        }
    }

    pub(crate) fn document(&self, uri: &Url) -> Option<SnapshotDocument<'_>> {
        let (file_id, source) = self.sources.iter().find(|(_, source)| &source.uri == uri)?;
        Some(SnapshotDocument {
            file_id: *file_id,
            file: self.result.files.get(file_id)?,
            line_index: &source.line_index,
        })
    }

    pub(crate) fn position(&self, uri: &Url, position: Position) -> Option<SnapshotPosition<'_>> {
        let document = self.document(uri)?;
        let offset = document.line_index.position_to_offset(position)?;
        Some(SnapshotPosition { document, offset })
    }

    pub(crate) fn source(&self, file_id: u32) -> Option<&SnapshotSource> {
        self.sources.get(&file_id)
    }

    /// On-disk path of a `go:` typedef file, if `file_id` names one.
    pub(crate) fn typedef_path(&self, file_id: u32) -> Option<&std::path::Path> {
        self.result.typedef_paths.get(&file_id).map(|p| p.as_path())
    }

    pub(crate) fn files(&self) -> &HashMap<u32, File> {
        &self.result.files
    }

    pub(crate) fn facts(&self) -> &Facts {
        &self.facts
    }

    pub(crate) fn definitions(&self) -> &HashMap<Symbol, Definition> {
        &self.result.definitions
    }
}
