use std::{
    collections::HashMap,
    fs::{read_dir, read_to_string},
    path::{Path, PathBuf},
};

use semantics::loader::{Files, Loader};
use semantics::store::ENTRY_MODULE_ID;

pub struct LocalFileSystem {
    search_paths: Vec<PathBuf>,
}

impl LocalFileSystem {
    pub fn new(cwd: &str) -> Self {
        let current_path = Path::new(cwd).to_path_buf();
        let stdlib_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("std");

        Self {
            search_paths: vec![current_path, stdlib_path],
        }
    }

    fn collect_files(&self, folder_path: &Path) -> Files {
        let Ok(entries) = read_dir(folder_path) else {
            return HashMap::default();
        };

        let mut files = HashMap::default();

        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "lis")
                && let Some(filename) = path.file_name().and_then(|s| s.to_str())
                && let Ok(source) = read_to_string(&path)
            {
                files.insert(filename.to_string(), source);
            }
        }

        files
    }
}

/// Translate module ID to filesystem path (entry module maps to current directory)
fn to_fs_path(folder_name: &str) -> &str {
    if folder_name == ENTRY_MODULE_ID {
        "."
    } else {
        folder_name
    }
}

pub fn collect_lis_filepaths_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = read_dir(dir) else {
        return files;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_lis_filepaths_recursive(&path));
        } else if path.extension().is_some_and(|e| e == "lis") {
            files.push(path);
        }
    }

    files
}

impl Loader for LocalFileSystem {
    fn scan_folder(&self, folder_name: &str) -> Files {
        let folder_name = to_fs_path(folder_name);
        for search_path in &self.search_paths {
            let folder_path = search_path.join(folder_name);
            let files = self.collect_files(&folder_path);

            if !files.is_empty() {
                return files;
            }
        }

        HashMap::default()
    }
}
