use rustc_hash::FxHashMap as HashMap;
use std::fs::{read_dir, read_to_string};
use std::path::{Path, PathBuf};

use semantics::loader::{DiscoveredModules, FileContent, Files, Loader};
use tokio::sync::{RwLock, RwLockMappedWriteGuard, RwLockWriteGuard};
use tower_lsp::lsp_types::Url;

use crate::paths::{ENTRY_MODULE_ID, module_id_to_dir, uri_to_module_file};
use crate::project::{ProjectConfig, find_project_root, resolve_standalone_root};

pub(crate) struct ProjectState {
    loader: RwLock<Option<OverlayLoader>>,
}

pub(crate) struct ProjectAnalysis {
    pub(crate) config: ProjectConfig,
    pub(crate) filename: String,
    pub(crate) external_test: bool,
    pub(crate) loader: AnalysisLoader,
}

impl ProjectState {
    pub(crate) fn new() -> Self {
        Self {
            loader: RwLock::new(None),
        }
    }

    pub(crate) async fn initialize(&self, config: ProjectConfig) {
        *self.loader.write().await = Some(OverlayLoader::new(config));
    }

    async fn loader_for(&self, uri: &Url) -> Option<RwLockMappedWriteGuard<'_, OverlayLoader>> {
        let mut loader = self.loader.write().await;
        if loader.is_none() {
            let path = uri.to_file_path().ok()?;
            let config = find_project_root(&path).unwrap_or_else(|| resolve_standalone_root(&path));
            *loader = Some(OverlayLoader::new(config));
        }
        Some(RwLockWriteGuard::map(loader, |loader| {
            loader.as_mut().expect("loader was initialized above")
        }))
    }

    pub(crate) async fn update_overlay(&self, uri: &Url, content: String) {
        let Some(mut project) = self.loader_for(uri).await else {
            return;
        };
        let Some((module_id, filename, external_test)) = uri_to_module_file(&project.config, uri)
        else {
            return;
        };
        project.set_overlay(external_test, &module_id, &filename, content);
    }

    pub(crate) async fn remove_overlay(&self, uri: &Url) -> bool {
        let mut project = self.loader.write().await;
        let Some(project) = project.as_mut() else {
            return false;
        };
        let Some((module_id, filename, external_test)) = uri_to_module_file(&project.config, uri)
        else {
            return false;
        };
        project.remove_overlay(external_test, &module_id, &filename);
        true
    }

    pub(crate) async fn for_analysis(&self, uri: &Url) -> Option<ProjectAnalysis> {
        let project = self.loader_for(uri).await?;
        let (module_id, filename, external_test) = uri_to_module_file(&project.config, uri)?;

        let (entry_module_path, external_test_root) = if external_test {
            (
                module_id_to_dir(&project.config, ENTRY_MODULE_ID),
                Some(module_id),
            )
        } else {
            let dir = uri
                .to_file_path()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| module_id_to_dir(&project.config, &module_id));
            (dir, None)
        };

        Some(ProjectAnalysis {
            config: project.config.clone(),
            filename,
            external_test,
            loader: project.for_analysis(entry_module_path, external_test_root),
        })
    }
}

pub(crate) struct OverlayLoader {
    config: ProjectConfig,
    overlays: HashMap<(bool, String, String), String>,
}

impl OverlayLoader {
    pub(crate) fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            overlays: HashMap::default(),
        }
    }

    pub(crate) fn set_overlay(
        &mut self,
        external_test: bool,
        module_id: &str,
        filename: &str,
        content: String,
    ) {
        self.overlays.insert(
            (external_test, module_id.to_string(), filename.to_string()),
            content,
        );
    }

    pub(crate) fn remove_overlay(&mut self, external_test: bool, module_id: &str, filename: &str) {
        self.overlays
            .remove(&(external_test, module_id.to_string(), filename.to_string()));
    }

    pub(crate) fn for_analysis(
        &self,
        entry_module_path: PathBuf,
        external_test_root: Option<String>,
    ) -> AnalysisLoader {
        AnalysisLoader {
            config: self.config.clone(),
            overlays: self.overlays.clone(),
            entry_module_path,
            external_test_root,
        }
    }
}

pub(crate) struct AnalysisLoader {
    config: ProjectConfig,
    overlays: HashMap<(bool, String, String), String>,
    entry_module_path: PathBuf,
    external_test_root: Option<String>,
}

impl AnalysisLoader {
    fn module_path(&self, module_id: &str) -> PathBuf {
        if module_id == ENTRY_MODULE_ID {
            self.entry_module_path.clone()
        } else {
            module_id_to_dir(&self.config, module_id)
        }
    }

    fn derive_module_id(&self, path: &Path) -> Option<String> {
        let source_root = self.config.source_root();
        if path == source_root {
            Some(ENTRY_MODULE_ID.to_string())
        } else {
            path.strip_prefix(source_root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        }
    }
}

impl Loader for AnalysisLoader {
    fn scan_folder(&self, module_id: &str) -> Files {
        let folder_path = self.module_path(module_id);
        let mut files = HashMap::default();

        if let Ok(entries) = read_dir(&folder_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(filename) = path.file_name().and_then(|s| s.to_str())
                    && filename.ends_with(".lis")
                    && let Ok(content) = read_to_string(&path)
                {
                    files.insert(
                        filename.to_string(),
                        FileContent::new(content, filename.to_string()),
                    );
                }
            }
        }

        let overlay_key = if module_id == ENTRY_MODULE_ID {
            self.derive_module_id(&self.entry_module_path)
                .map(|id| (false, id))
        } else {
            let external = self.external_test_root.as_deref() == Some(module_id);
            Some((external, module_id.to_string()))
        };

        if let Some((external_test, overlay_module)) = overlay_key {
            for ((_, _, filename), content) in self
                .overlays
                .iter()
                .filter(|((ext, module, _), _)| *ext == external_test && module == &overlay_module)
            {
                files.insert(
                    filename.clone(),
                    FileContent::new(content.clone(), filename.clone()),
                );
            }
        }

        files
    }

    fn discover_modules(&self) -> DiscoveredModules {
        DiscoveredModules {
            production_modules: vec![ENTRY_MODULE_ID.to_string()],
            internal_test_roots: Vec::new(),
            external_test_roots: self.external_test_root.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn project_state_owns_config_and_overlay_lifecycle() {
        let temp = tempdir().unwrap();
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();
        let first_path = first_root.join("main.lis");
        let second_path = second_root.join("main.lis");
        fs::write(&first_path, "disk").unwrap();
        fs::write(&second_path, "other").unwrap();
        let first_uri = Url::from_file_path(&first_path).unwrap();
        let second_uri = Url::from_file_path(&second_path).unwrap();

        let project = ProjectState::new();
        project
            .update_overlay(&first_uri, "memory".to_string())
            .await;

        let analysis = project.for_analysis(&first_uri).await.unwrap();
        assert_eq!(
            analysis.loader.scan_folder(ENTRY_MODULE_ID)["main.lis"].source,
            "memory"
        );
        assert!(project.for_analysis(&second_uri).await.is_none());

        assert!(project.remove_overlay(&first_uri).await);
        let analysis = project.for_analysis(&first_uri).await.unwrap();
        assert_eq!(
            analysis.loader.scan_folder(ENTRY_MODULE_ID)["main.lis"].source,
            "disk"
        );
    }
}
