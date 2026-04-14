use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs::{read_dir, read_to_string, remove_file},
    io,
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

pub fn prune_orphan_go_files(target_dir: &Path, produced: &[&str]) -> io::Result<()> {
    let mut produced_by_dir: HashMap<&Path, HashSet<&OsStr>> = HashMap::new();
    for rel in produced {
        let rel = Path::new(rel);
        let Some(name) = rel.file_name() else {
            continue;
        };
        let parent = rel.parent().unwrap_or(Path::new(""));
        produced_by_dir.entry(parent).or_default().insert(name);
    }

    for (rel_parent, names) in &produced_by_dir {
        let dir = target_dir.join(rel_parent);
        let entries = match read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };

        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name();
            if Path::new(&name).extension().is_some_and(|ext| ext == "go")
                && !names.contains(name.as_os_str())
            {
                remove_file(entry.path())?;
            }
        }
    }

    Ok(())
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
