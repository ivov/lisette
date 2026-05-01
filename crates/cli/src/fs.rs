use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs::{File, read_dir, read_to_string, remove_file},
    io::{self, BufRead, BufReader},
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
                && !is_generated_go_file(&entry.path())
            {
                remove_file(entry.path())?;
            }
        }
    }

    Ok(())
}

/// Detects the Go-standard generated-code marker so codegen tool output
/// (e.g. `wire_gen.go`, `*.pb.go`) is not pruned.
fn is_generated_go_file(path: &Path) -> bool {
    const MAX_HEADER_LINES: usize = 64;

    // I/O failure → preserve. Leaving an orphan is cheaper than deleting
    // a file we cannot inspect.
    let Ok(file) = File::open(path) else {
        return true;
    };
    let reader = BufReader::new(file);

    for line in reader.lines().take(MAX_HEADER_LINES) {
        let Ok(line) = line else {
            return true;
        };
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with("//") {
            return false;
        }
        if is_generated_marker(trimmed) {
            return true;
        }
    }

    false
}

fn is_generated_marker(line: &str) -> bool {
    const PREFIX: &str = "// Code generated ";
    const SUFFIX: &str = " DO NOT EDIT.";
    line.len() >= PREFIX.len() + SUFFIX.len() && line.starts_with(PREFIX) && line.ends_with(SUFFIX)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        stdfs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn preserves_file_with_marker_on_first_line() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "wire_gen.go",
            "// Code generated by Wire. DO NOT EDIT.\n\npackage main\n",
        );
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(tmp.path().join("wire_gen.go").exists());
        assert!(tmp.path().join("main.go").exists());
    }

    #[test]
    fn preserves_file_with_marker_after_build_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// Code generated by Wire. DO NOT EDIT.\n\
                       \n\
                       //go:generate go run -mod=mod github.com/google/wire/cmd/wire\n\
                       //go:build !wireinject\n\
                       // +build !wireinject\n\
                       \n\
                       package main\n";
        write_file(tmp.path(), "wire_gen.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(tmp.path().join("wire_gen.go").exists());
    }

    #[test]
    fn preserves_when_marker_is_not_on_first_line() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "//go:build !wireinject\n\
                       // +build !wireinject\n\
                       \n\
                       // Code generated by Wire. DO NOT EDIT.\n\
                       \n\
                       package main\n";
        write_file(tmp.path(), "wire_gen.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(tmp.path().join("wire_gen.go").exists());
    }

    #[test]
    fn prunes_file_without_marker() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "stale.go", "package main\n\nfunc Old() {}\n");
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(!tmp.path().join("stale.go").exists());
    }

    #[test]
    fn prunes_when_marker_appears_only_after_package_clause() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "package main\n\
                       \n\
                       // Code generated by Wire. DO NOT EDIT.\n\
                       func F() {}\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn prunes_partial_marker_missing_do_not_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// Code generated by Wire.\n\npackage main\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn prunes_marker_with_wrong_casing() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// code generated by Wire. DO NOT EDIT.\n\npackage main\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn prunes_marker_with_collapsed_spaces() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// Code generated DO NOT EDIT.\n\npackage main\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn produced_files_are_kept_regardless_of_marker() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(tmp.path().join("main.go").exists());
    }

    #[test]
    fn non_go_files_are_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "go.mod", "module foo\n");
        write_file(tmp.path(), "go.sum", "");
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"]).unwrap();

        assert!(tmp.path().join("go.mod").exists());
        assert!(tmp.path().join("go.sum").exists());
    }
}
