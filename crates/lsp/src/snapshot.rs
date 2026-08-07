use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use std::sync::Arc;

use crate::protocol::{Position, Url};
use passes::Analysis;
use semantics::facts::{BindingFact, Usage};
use syntax::ast::BindingId;
use syntax::program::{Definition, File};
use syntax::types::Symbol;

use crate::imports::{Importable, PackageResolver};
use crate::paths::{ENTRY_PACKAGE_ID, package_file_to_path};
use crate::position::LineIndex;
use crate::project::ProjectConfig;

pub(crate) struct AnalysisSnapshot {
    pub(crate) analysis: Analysis,
    sources: HashMap<u32, SnapshotSource>,
    packages: PackageResolver,
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
        analysis: Analysis,
        config: &ProjectConfig,
        analyzed_uri: &Url,
        external_test: bool,
        packages: PackageResolver,
    ) -> Self {
        let mut sources = HashMap::default();

        let analyzed_path = analyzed_uri.to_file_path().ok();
        let analyzed_filename = analyzed_path
            .as_ref()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()));
        let analyzed_dir = analyzed_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        for (file_id, file) in &analysis.emit_input.files {
            let uri = if !external_test && file.package_id == ENTRY_PACKAGE_ID {
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
            } else if let Some(typedef_path) = &file.source_path {
                // The synthetic `file.name` for go: typedefs does not match the
                // on-disk filename, use the path the locator captured.
                match Url::from_file_path(typedef_path) {
                    Ok(uri) => uri,
                    Err(_) => continue,
                }
            } else if file.package_id.starts_with("go:") || file.package_id == "prelude" {
                // Embedded typedef with no recorded on-disk path; nothing to navigate to.
                continue;
            } else {
                let path = package_file_to_path(config, &file.package_id, &file.name);
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
            analysis,
            sources,
            packages,
        }
    }

    pub(crate) fn importable_packages(&self) -> Arc<Vec<Importable>> {
        self.packages.packages()
    }

    pub(crate) fn document(&self, uri: &Url) -> Option<SnapshotDocument<'_>> {
        let (file_id, source) = self.sources.iter().find(|(_, source)| &source.uri == uri)?;
        Some(SnapshotDocument {
            file_id: *file_id,
            file: self.analysis.emit_input.files.get(file_id)?,
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
        self.analysis
            .emit_input
            .files
            .get(&file_id)?
            .source_path
            .as_deref()
    }

    pub(crate) fn files(&self) -> &HashMap<u32, File> {
        &self.analysis.emit_input.files
    }

    pub(crate) fn bindings(&self) -> &HashMap<BindingId, BindingFact> {
        self.analysis.bindings()
    }

    pub(crate) fn usages(&self) -> &HashSet<Usage> {
        self.analysis.usages()
    }

    pub(crate) fn definitions(&self) -> &HashMap<Symbol, Definition> {
        &self.analysis.emit_input.definitions
    }

    pub(crate) fn has_parse_errors(&self) -> bool {
        self.analysis.has_parse_errors()
    }
}
