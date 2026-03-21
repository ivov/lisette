use std::collections::HashMap;

use semantics::loader::{Files, Loader};

#[derive(Clone, Default)]
pub struct MockFileSystem {
    folders: HashMap<String, Files>,
}

impl MockFileSystem {
    pub fn new() -> Self {
        Self {
            folders: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, folder: &str, filename: &str, content: &str) {
        self.folders
            .entry(folder.to_string())
            .or_default()
            .insert(filename.to_string(), content.to_string());
    }

    pub(super) fn get_folders(&self) -> Vec<String> {
        self.folders.keys().cloned().collect()
    }
}

impl Loader for MockFileSystem {
    fn scan_folder(&self, folder: &str) -> Files {
        self.folders.get(folder).cloned().unwrap_or_default()
    }
}
