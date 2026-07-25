use rustc_hash::FxHashMap as HashMap;
use std::fs::{read_dir, read_to_string};
use std::path::{Path, PathBuf};

use semantics::loader::{DiscoveredModules, FileContent, Files, Loader};

use crate::paths::{ENTRY_MODULE_ID, module_id_to_dir};
use crate::project::ProjectConfig;

#[derive(Clone)]
pub(crate) struct OverlayLoader {
    config: ProjectConfig,
    overlays: HashMap<(bool, String), HashMap<String, String>>,
    /// Override path for ENTRY_MODULE_ID (set when analyzing submodule files).
    entry_module_path_override: Option<PathBuf>,
    /// The external test module under analysis, surfaced as a discovered root.
    external_test_root: Option<String>,
}

impl OverlayLoader {
    pub(crate) fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            overlays: HashMap::default(),
            entry_module_path_override: None,
            external_test_root: None,
        }
    }

    pub(crate) fn set_config(&mut self, config: ProjectConfig) {
        self.config = config;
    }

    pub(crate) fn set_overlay(
        &mut self,
        external_test: bool,
        module_id: &str,
        filename: &str,
        content: String,
    ) {
        self.overlays
            .entry((external_test, module_id.to_string()))
            .or_default()
            .insert(filename.to_string(), content);
    }

    pub(crate) fn remove_overlay(&mut self, external_test: bool, module_id: &str, filename: &str) {
        if let Some(module_overlays) = self
            .overlays
            .get_mut(&(external_test, module_id.to_string()))
        {
            module_overlays.remove(filename);
        }
    }

    pub(crate) fn set_entry_module_path(&mut self, path: Option<PathBuf>) {
        self.entry_module_path_override = path;
    }

    pub(crate) fn set_external_test_root(&mut self, module_id: Option<String>) {
        self.external_test_root = module_id;
    }

    fn module_path(&self, module_id: &str) -> PathBuf {
        if module_id == ENTRY_MODULE_ID
            && let Some(ref override_path) = self.entry_module_path_override
        {
            return override_path.clone();
        }

        module_id_to_dir(&self.config, module_id)
    }
}

impl Loader for OverlayLoader {
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
            let derived = match &self.entry_module_path_override {
                Some(override_path) => self.derive_module_id(override_path),
                None => Some(ENTRY_MODULE_ID.to_string()),
            };
            derived.map(|id| (false, id))
        } else {
            let external = self.external_test_root.as_deref() == Some(module_id);
            Some((external, module_id.to_string()))
        };

        if let Some(key) = overlay_key
            && let Some(module_overlays) = self.overlays.get(&key)
        {
            for (filename, content) in module_overlays {
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

impl OverlayLoader {
    fn derive_module_id(&self, path: &Path) -> Option<String> {
        if self.config.standalone_mode {
            if path == self.config.root {
                Some(ENTRY_MODULE_ID.to_string())
            } else {
                path.strip_prefix(&self.config.root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
            }
        } else {
            if let Ok(relative) = path.strip_prefix(&self.config.root) {
                let id = crate::paths::module_id_from_components(relative);
                if semantics::loader::is_external_test_module(&id) {
                    return Some(id);
                }
            }

            let src_dir = self.config.root.join("src");
            if path == src_dir {
                Some(ENTRY_MODULE_ID.to_string())
            } else {
                path.strip_prefix(&src_dir)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
            }
        }
    }
}
