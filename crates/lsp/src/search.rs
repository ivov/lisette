use std::collections::HashSet;
use std::sync::Arc;
use std::{fs, io};

use deps::TypedefLocator;
use diagnostics::LocalSink;
use semantics::package_graph::{PackageGraphOptions, Roots, build_package_graph};
use semantics::store::Store;
use semantics::{AnalysisScope, ProjectKind};
use syntax::ast::Span;
use syntax::program::is_internal_package_id;

use crate::loader::AnalysisLoader;
use crate::paths::{ENTRY_PACKAGE_ID, uri_to_package_file};
use crate::project::ProjectConfig;
use crate::protocol::{Error, Location, RpcResult, Url};
use crate::snapshot::AnalysisSnapshot;
use crate::state::{AnalysisKey, SharedState};
use crate::usage_locations;

impl SharedState {
    pub(crate) fn reference_locations(
        &self,
        uri: &Url,
        origin: &Arc<AnalysisSnapshot>,
        definition: Span,
        require_complete: bool,
    ) -> RpcResult<Vec<Location>> {
        let config = self
            .projects
            .config_for(uri)
            .ok_or_else(Error::content_modified)?;
        let definition_source = origin
            .source(definition.file_id)
            .ok_or_else(incomplete_search)?;
        let definition_uri = &definition_source.uri;
        if config.is_script() {
            let mut locations =
                usage_locations(self.open_document_snapshots(), definition_uri, definition);
            if definition_uri
                .to_file_path()
                .is_ok_and(|path| deps::is_generated_typedef_path(&path))
            {
                let documents: Vec<_> = self.workspace().documents.keys().cloned().collect();
                let mut projects = HashSet::new();
                for document in documents {
                    if let Some(config) = self.projects.config_for(&document)
                        && !config.is_script()
                        && projects.insert(config.root().to_path_buf())
                    {
                        locations.extend(self.reference_locations(
                            &document,
                            origin,
                            definition,
                            require_complete,
                        )?);
                    }
                }
            }
            return Ok(locations);
        }
        let target = uri_to_package_file(&config, definition_uri);
        let external_definition = target.is_none();
        let target_package = target
            .map(|(package, _, _)| package)
            .or_else(|| {
                origin
                    .files()
                    .get(&definition.file_id)
                    .map(|file| file.package_id.clone())
            })
            .ok_or_else(incomplete_search)?;
        let local = origin
            .bindings()
            .values()
            .any(|binding| binding.span == definition);
        let (generation, project) = {
            let workspace = self.workspace();
            if require_complete
                && workspace.documents.get(uri).is_some_and(|document| {
                    origin
                        .document(uri)
                        .is_none_or(|analyzed| analyzed.file.source != document.content())
                })
            {
                return Err(Error::content_modified());
            }
            if local {
                if require_complete && origin.analysis.dependencies().is_none() {
                    return Err(incomplete_search());
                }
                return Ok(usage_locations(
                    [Arc::clone(origin)],
                    definition_uri,
                    definition,
                ));
            }
            let root = AnalysisKey::Package {
                project_root: config.root().to_path_buf(),
                external_test: false,
                package_id: ENTRY_PACKAGE_ID.to_string(),
            };
            (
                workspace.generation(),
                self.projects
                    .for_key(&root)
                    .ok_or_else(Error::content_modified)?,
            )
        };
        let manifest_path = config.root().join("lisette.toml");
        let manifest = fs::read(&manifest_path).map_err(search_io_error)?;
        let loader = project.loader.capture_project().map_err(search_io_error)?;
        let keys = loader.project_keys().map_err(search_io_error)?;
        let locator = TypedefLocator::from_project(config.root()).unwrap_or_default();
        let sink = LocalSink::new();
        let graph = build_package_graph(
            &mut Store::new(),
            Roots {
                primary: vec![ENTRY_PACKAGE_ID.into()],
                additional: keys
                    .iter()
                    .filter_map(|key| match key {
                        AnalysisKey::Package { package_id, .. } => Some(package_id.as_str().into()),
                        _ => None,
                    })
                    .collect(),
            },
            PackageGraphOptions {
                loader: Some(&loader),
                sink: &sink,
                scope: &AnalysisScope::Project(config.root().to_path_buf()),
                locator: &locator,
                include_tests: true,
                project_kind: if config.root().join("src/main.lis").exists() {
                    ProjectKind::Binary
                } else {
                    ProjectKind::Library
                },
            },
        );
        let incomplete = sink.has_errors()
            || !graph.cycles.is_empty()
            || graph.files.values().flatten().any(|file| {
                !syntax::build_ast(&file.source, file.file_id)
                    .errors
                    .is_empty()
            });
        if require_complete && incomplete {
            return Err(incomplete_search());
        }
        let relevant = graph
            .dependencies
            .reverse_dependency_closure(&target_package);
        let mut locations = Vec::new();
        for key in keys {
            let AnalysisKey::Package { package_id, .. } = &key else {
                continue;
            };
            if !incomplete && !external_definition && !relevant.contains(package_id.as_str()) {
                continue;
            }
            self.check_search_generation(generation)?;
            let current = self.workspace().snapshot(&key);
            let current =
                current.filter(|snapshot| snapshot_matches(&loader, &config, &key, snapshot));
            let snapshot = match current {
                Some(snapshot) => snapshot,
                None => self
                    .analyze_search_package(&key, loader.focus(&key).ok_or_else(incomplete_search)?)
                    .map_err(|_| incomplete_search())?,
            };
            if require_complete
                && (snapshot.analysis.dependencies().is_none()
                    || snapshot.analysis.errors().iter().any(|error| {
                        !matches!(
                            error.code_str(),
                            Some("resolve.manifest_error" | "infer.mismatched_return_value")
                        )
                    }))
            {
                return Err(incomplete_search());
            }
            if let Some(document) = snapshot.document(definition_uri)
                && origin
                    .document(definition_uri)
                    .is_none_or(|original| original.file.source != document.file.source)
            {
                return Err(Error::content_modified());
            }
            locations.extend(usage_locations([snapshot], definition_uri, definition));
        }
        self.check_search_generation(generation)?;
        if !loader.unchanged().map_err(search_io_error)?
            || fs::read(&manifest_path).map_err(search_io_error)? != manifest
        {
            return Err(Error::content_modified());
        }
        self.check_search_generation(generation)?;
        Ok(locations)
    }

    fn check_search_generation(&self, generation: u64) -> RpcResult<()> {
        if self.workspace().generation() == generation {
            Ok(())
        } else {
            Err(Error::content_modified())
        }
    }
}

fn snapshot_matches(
    loader: &AnalysisLoader,
    config: &ProjectConfig,
    key: &AnalysisKey,
    snapshot: &AnalysisSnapshot,
) -> bool {
    if snapshot.analysis.dependencies().is_none() {
        return false;
    }
    loader.captured_files().all(|(directory, files)| {
        files.iter().all(|(name, content)| {
            if name.ends_with(".d.lis") || name.ends_with("_test.lis") {
                return true;
            }
            let Ok(uri) = Url::from_file_path(directory.join(name)) else {
                return false;
            };
            let Some((package_id, _, external_test)) = uri_to_package_file(config, &uri) else {
                return false;
            };
            let changed = AnalysisKey::Package {
                project_root: config.root().to_path_buf(),
                external_test,
                package_id,
            };
            !snapshot.depends_on(key, &changed)
                || snapshot
                    .document(&uri)
                    .is_some_and(|document| document.file.source == content.source)
        })
    }) && snapshot.files().iter().all(|(file_id, file)| {
        if is_internal_package_id(&file.package_id) {
            return true;
        }
        let Some(source) = snapshot.source(*file_id) else {
            return true;
        };
        let Ok(path) = source.uri.to_file_path() else {
            return false;
        };
        if uri_to_package_file(config, &source.uri).is_none() {
            return true;
        }
        loader.captured_source(&path) == Some(file.source.as_str())
    })
}

fn incomplete_search() -> Error {
    Error::request_failed(
        "Cannot complete the reference search. Fix errors in the affected project and try again.",
    )
}

fn search_io_error(error: io::Error) -> Error {
    Error::request_failed(format!("Cannot read all project files: {error}"))
}
