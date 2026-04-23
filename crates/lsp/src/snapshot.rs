use rustc_hash::FxHashMap as HashMap;

use diagnostics::SemanticResult;
use semantics::facts::Facts;
use syntax::program::{Definition, File};
use syntax::types::Symbol;
use tower_lsp::lsp_types::Url;

use crate::paths::{ENTRY_MODULE_ID, module_file_to_path};
use crate::position::LineIndex;
use crate::project::ProjectConfig;

pub(crate) struct AnalysisSnapshot {
    pub(crate) result: SemanticResult,
    facts: Facts,
    pub(crate) has_parse_errors: bool,
    uri_to_id: HashMap<Url, u32>,
    id_to_uri: HashMap<u32, Url>,
    line_indexes: HashMap<u32, LineIndex>,
}

// SAFETY: AnalysisSnapshot is immutable after construction. Non-Send/Sync types
// it transitively contains are never mutated after analyze() returns.
unsafe impl Send for AnalysisSnapshot {}
unsafe impl Sync for AnalysisSnapshot {}

impl AnalysisSnapshot {
    pub(crate) fn new(
        result: SemanticResult,
        facts: Facts,
        has_parse_errors: bool,
        config: &ProjectConfig,
        analyzed_uri: &Url,
    ) -> Self {
        let mut uri_to_id = HashMap::default();
        let mut id_to_uri = HashMap::default();
        let mut line_indexes = HashMap::default();

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
            } else {
                let path = module_file_to_path(config, &file.module_id, &file.name);
                match Url::from_file_path(&path) {
                    Ok(uri) => uri,
                    Err(_) => continue,
                }
            };

            uri_to_id.insert(uri.clone(), *file_id);
            id_to_uri.insert(*file_id, uri);
            line_indexes.insert(*file_id, LineIndex::new(&file.source));
        }

        Self {
            result,
            facts,
            has_parse_errors,
            uri_to_id,
            id_to_uri,
            line_indexes,
        }
    }

    pub(crate) fn get_file_id(&self, uri: &Url) -> Option<u32> {
        self.uri_to_id.get(uri).copied()
    }

    pub(crate) fn get_uri(&self, file_id: u32) -> Option<&Url> {
        self.id_to_uri.get(&file_id)
    }

    pub(crate) fn get_line_index(&self, file_id: u32) -> Option<&LineIndex> {
        self.line_indexes.get(&file_id)
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
