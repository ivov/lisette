use rustc_hash::FxHashMap as HashMap;
use std::fs::{read_dir, read_to_string};
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock, RwLockWriteGuard};

use crate::protocol::Url;
use semantics::loader::{DiscoveredPackages, FileContent, Files, Loader};

use crate::paths::{ENTRY_PACKAGE_ID, package_id_to_dir, uri_to_package_file};
use crate::project::{ProjectConfig, find_project_root, resolve_script_root};

pub(crate) struct ProjectState {
    loader: RwLock<Option<OverlayLoader>>,
}

pub(crate) struct ProjectAnalysis {
    pub(crate) config: ProjectConfig,
    pub(crate) package_id: String,
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

    pub(crate) fn initialize(&self, config: ProjectConfig) {
        *self.loader.write().unwrap_or_else(PoisonError::into_inner) =
            Some(OverlayLoader::new(config));
    }

    fn loader_for(&self, uri: &Url) -> Option<RwLockWriteGuard<'_, Option<OverlayLoader>>> {
        let mut loader = self.loader.write().unwrap_or_else(PoisonError::into_inner);
        if loader.is_none() {
            let path = uri.to_file_path().ok()?;
            let config = find_project_root(&path).unwrap_or_else(|| resolve_script_root(&path));
            *loader = Some(OverlayLoader::new(config));
        }
        Some(loader)
    }

    pub(crate) fn update_overlay(&self, uri: &Url, content: String) {
        let Some(mut guard) = self.loader_for(uri) else {
            return;
        };
        let project = guard.as_mut().expect("loader_for initializes the loader");
        let Some((package_id, filename, external_test)) = uri_to_package_file(&project.config, uri)
        else {
            return;
        };
        project.set_overlay(external_test, &package_id, &filename, content);
    }

    pub(crate) fn remove_overlay(&self, uri: &Url) -> bool {
        let mut project = self.loader.write().unwrap_or_else(PoisonError::into_inner);
        let Some(project) = project.as_mut() else {
            return false;
        };
        let Some((package_id, filename, external_test)) = uri_to_package_file(&project.config, uri)
        else {
            return false;
        };
        project.remove_overlay(external_test, &package_id, &filename);
        true
    }

    pub(crate) fn for_analysis(&self, uri: &Url) -> Option<ProjectAnalysis> {
        let guard = self.loader_for(uri)?;
        let project = guard.as_ref().expect("loader_for initializes the loader");
        let (package_id, filename, external_test) = uri_to_package_file(&project.config, uri)?;

        let (entry_package_path, external_test_root) = if external_test {
            (
                package_id_to_dir(&project.config, ENTRY_PACKAGE_ID),
                Some(package_id.clone()),
            )
        } else {
            let dir = uri
                .to_file_path()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| package_id_to_dir(&project.config, &package_id));
            (dir, None)
        };

        Some(ProjectAnalysis {
            config: project.config.clone(),
            package_id,
            filename,
            external_test,
            loader: project.for_analysis(entry_package_path, external_test_root),
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
        package_id: &str,
        filename: &str,
        content: String,
    ) {
        self.overlays.insert(
            (external_test, package_id.to_string(), filename.to_string()),
            content,
        );
    }

    pub(crate) fn remove_overlay(&mut self, external_test: bool, package_id: &str, filename: &str) {
        self.overlays
            .remove(&(external_test, package_id.to_string(), filename.to_string()));
    }

    pub(crate) fn for_analysis(
        &self,
        entry_package_path: PathBuf,
        external_test_root: Option<String>,
    ) -> AnalysisLoader {
        AnalysisLoader {
            config: self.config.clone(),
            overlays: self.overlays.clone(),
            entry_package_path,
            external_test_root,
        }
    }
}

pub(crate) struct AnalysisLoader {
    config: ProjectConfig,
    overlays: HashMap<(bool, String, String), String>,
    entry_package_path: PathBuf,
    external_test_root: Option<String>,
}

impl AnalysisLoader {
    fn package_path(&self, package_id: &str) -> PathBuf {
        if package_id == ENTRY_PACKAGE_ID {
            self.entry_package_path.clone()
        } else {
            package_id_to_dir(&self.config, package_id)
        }
    }

    fn derive_package_id(&self, path: &Path) -> Option<String> {
        let source_root = self.config.source_root();
        if path == source_root {
            Some(ENTRY_PACKAGE_ID.to_string())
        } else {
            path.strip_prefix(source_root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        }
    }
}

impl Loader for AnalysisLoader {
    fn scan_folder(&self, package_id: &str) -> Files {
        let folder_path = self.package_path(package_id);
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

        let overlay_key = if package_id == ENTRY_PACKAGE_ID {
            self.derive_package_id(&self.entry_package_path)
                .map(|id| (false, id))
        } else {
            let external = self.external_test_root.as_deref() == Some(package_id);
            Some((external, package_id.to_string()))
        };

        if let Some((external_test, overlay_package)) = overlay_key {
            for ((_, _, filename), content) in
                self.overlays.iter().filter(|((ext, package, _), _)| {
                    *ext == external_test && package == &overlay_package
                })
            {
                files.insert(
                    filename.clone(),
                    FileContent::new(content.clone(), filename.clone()),
                );
            }
        }

        files
    }

    fn discover_packages(&self) -> DiscoveredPackages {
        let mut discovered = DiscoveredPackages::default();
        discovered.add_production(ENTRY_PACKAGE_ID.to_string(), false);
        if let Some(package_id) = &self.external_test_root {
            discovered.add_external_test_root(package_id.clone());
        }
        discovered
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn project_state_owns_config_and_overlay_lifecycle() {
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
        project.update_overlay(&first_uri, "memory".to_string());

        let analysis = project.for_analysis(&first_uri).unwrap();
        assert_eq!(
            analysis.loader.scan_folder(ENTRY_PACKAGE_ID)["main.lis"].source,
            "memory"
        );
        assert!(project.for_analysis(&second_uri).is_none());

        assert!(project.remove_overlay(&first_uri));
        let analysis = project.for_analysis(&first_uri).unwrap();
        assert_eq!(
            analysis.loader.scan_folder(ENTRY_PACKAGE_ID)["main.lis"].source,
            "disk"
        );
    }
}
