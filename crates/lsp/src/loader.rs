use rustc_hash::FxHashMap as HashMap;
use std::fs::{read_dir, read_to_string};
use std::path::{Path, PathBuf};

use semantics::loader::{Files, Loader};

use crate::paths::{ENTRY_MODULE_ID, module_id_to_dir};
use crate::project::ProjectConfig;

#[derive(Clone)]
pub(crate) struct OverlayLoader {
    config: ProjectConfig,
    /// module_id -> filename -> content (in-memory overrides)
    overlays: HashMap<String, HashMap<String, String>>,
    /// Override path for ENTRY_MODULE_ID (set when analyzing submodule files).
    entry_module_path_override: Option<PathBuf>,
}

impl OverlayLoader {
    pub(crate) fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            overlays: HashMap::default(),
            entry_module_path_override: None,
        }
    }

    pub(crate) fn set_config(&mut self, config: ProjectConfig) {
        self.config = config;
    }

    pub(crate) fn set_overlay(&mut self, module_id: &str, filename: &str, content: String) {
        self.overlays
            .entry(module_id.to_string())
            .or_default()
            .insert(filename.to_string(), content);
    }

    pub(crate) fn remove_overlay(&mut self, module_id: &str, filename: &str) {
        if let Some(module_overlays) = self.overlays.get_mut(module_id) {
            module_overlays.remove(filename);
        }
    }

    pub(crate) fn set_entry_module_path(&mut self, path: Option<PathBuf>) {
        self.entry_module_path_override = path;
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
                    files.insert(filename.to_string(), content);
                }
            }
        }

        if module_id == ENTRY_MODULE_ID {
            if let Some(ref override_path) = self.entry_module_path_override {
                if let Some(actual_module_id) = self.derive_module_id(override_path)
                    && let Some(module_overlays) = self.overlays.get(&actual_module_id)
                {
                    for (filename, content) in module_overlays {
                        files.insert(filename.clone(), content.clone());
                    }
                }
            } else if let Some(module_overlays) = self.overlays.get(ENTRY_MODULE_ID) {
                for (filename, content) in module_overlays {
                    files.insert(filename.clone(), content.clone());
                }
            }
        } else if let Some(module_overlays) = self.overlays.get(module_id) {
            for (filename, content) in module_overlays {
                files.insert(filename.clone(), content.clone());
            }
        }

        files
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
