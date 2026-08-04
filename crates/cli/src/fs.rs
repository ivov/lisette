use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs::{File, read_dir, read_to_string, remove_file},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};

use semantics::cache::cache_file_name;
use semantics::loader::{DiscoveredPackages, FileContent, Files, Loader};
use semantics::path::DisplayPathBase;
use semantics::store::ENTRY_PACKAGE_ID;

pub use semantics::path::relative_to_cwd;

pub struct LocalFileSystem {
    search_paths: Vec<(PathBuf, DisplayPathBase)>,
    project_root: Option<(PathBuf, DisplayPathBase)>,
    scanned_sources: Option<Vec<PathBuf>>,
    scanned_test_sources: Option<Vec<PathBuf>>,
}

impl LocalFileSystem {
    pub fn new(cwd: &str, project_root: Option<&Path>) -> Self {
        let current_path = Path::new(cwd).to_path_buf();
        let stdlib_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("std");
        let project_root = project_root.map(|root| {
            let display_base = DisplayPathBase::new(root);
            (root.to_path_buf(), display_base)
        });

        Self {
            search_paths: [current_path, stdlib_path]
                .into_iter()
                .map(|path| {
                    let display_base = DisplayPathBase::new(&path);
                    (path, display_base)
                })
                .collect(),
            project_root,
            scanned_sources: None,
            scanned_test_sources: None,
        }
    }

    /// Build a loader rooted at `src_dir` whose package discovery reuses `sources`
    /// (the `.lis` files under that directory) and `test_sources` (the `.lis`
    /// files under the project's `tests/` directory).
    pub fn with_scanned_sources(
        src_dir: &Path,
        project_root: Option<&Path>,
        sources: Vec<PathBuf>,
        test_sources: Vec<PathBuf>,
    ) -> Self {
        let mut fs = Self::new(src_dir.to_str().unwrap_or("."), project_root);
        fs.scanned_sources = Some(sources);
        fs.scanned_test_sources = Some(test_sources);
        fs
    }

    fn discover_external_test_roots(&self) -> Vec<String> {
        let Some((project_root, _)) = &self.project_root else {
            return Vec::new();
        };
        let tests_root = project_root.join(semantics::loader::EXTERNAL_TESTS_DIR);
        let walked;
        let test_sources = match &self.scanned_test_sources {
            Some(test_sources) => test_sources.as_slice(),
            None => {
                walked = collect_lis_filepaths_recursive(&tests_root);
                &walked
            }
        };
        let mut dirs: HashSet<PathBuf> = HashSet::default();
        for path in test_sources {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".test.lis"))
                && let Some(dir) = path.parent()
            {
                dirs.insert(dir.to_path_buf());
            }
        }
        dirs.iter()
            .filter_map(|dir| dir.strip_prefix(project_root).ok())
            .map(package_id_from_rel)
            .collect()
    }

    fn collect_files(&self, folder_path: &Path, fs_name: &str, base: &DisplayPathBase) -> Files {
        let Ok(entries) = read_dir(folder_path) else {
            return HashMap::default();
        };

        let mut files = HashMap::default();

        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "lis")
                && let Ok(source) = read_to_string(&path)
                && let Some(name) = path.file_name().and_then(|s| s.to_str())
            {
                let display_path = base
                    .relative(&Path::new(fs_name).join(name))
                    .unwrap_or_else(|| name.to_string());
                files.insert(name.to_string(), FileContent::new(source, display_path));
            }
        }

        files
    }
}

fn to_fs_path(folder_name: &str) -> &str {
    if folder_name == ENTRY_PACKAGE_ID {
        ""
    } else {
        folder_name
    }
}

/// Removes stale Go output from `target/`: stale files within live packages, and the
/// emitted directories of packages that left the dep graph. `emitted` is the manifest
/// of lisette-written `.go` files, so unrelated content (e.g. `vendor/`) is untouched.
pub fn prune_orphan_go_files(
    target_dir: &Path,
    produced: &[&str],
    emitted: &[&str],
    live_packages: &[String],
) -> io::Result<()> {
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

    let (live_dirs, ancestor_dirs) = live_package_dirs(live_packages);
    for dir in emitted_package_dirs(emitted) {
        if live_dirs.contains(&dir) {
            continue;
        }
        let path = target_dir.join(&dir);
        // A dir with a live subpackage keeps its dir and sheds only `.go`. A leaf goes entirely.
        if ancestor_dirs.contains(&dir) {
            remove_direct_go_files(&path)?;
        } else {
            remove_dir_all_if_present(&path)?;
        }
    }
    prune_orphan_caches(target_dir, live_packages)?;

    Ok(())
}

/// Removes root `.go` a library did not produce. `prune_orphan_go_files` skips the root.
pub fn prune_stale_root_go(target_dir: &Path, produced: &[&str]) -> io::Result<()> {
    let produced_root: HashSet<&OsStr> = produced
        .iter()
        .map(Path::new)
        .filter(|p| {
            p.parent()
                .is_none_or(|parent| parent.as_os_str().is_empty())
        })
        .filter_map(|p| p.file_name())
        .collect();

    let entries = match read_dir(target_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        if Path::new(&name).extension().is_some_and(|ext| ext == "go")
            && !produced_root.contains(name.as_os_str())
            && !is_generated_go_file(&entry.path())
        {
            remove_file(entry.path())?;
        }
    }

    Ok(())
}

/// Directories lisette emitted Go output into (root entry-package files excluded).
fn emitted_package_dirs(emitted: &[&str]) -> HashSet<String> {
    let mut dirs = HashSet::new();
    for file in emitted {
        if let Some(parent) = Path::new(file).parent().and_then(|p| p.to_str())
            && !parent.is_empty()
        {
            dirs.insert(parent.to_string());
        }
    }
    dirs
}

/// Returns (live package dirs, ancestor-only dirs); a dir that is itself live wins.
fn live_package_dirs(live_packages: &[String]) -> (HashSet<String>, HashSet<String>) {
    let mut live = HashSet::new();
    let mut ancestors = HashSet::new();
    for package in live_packages {
        if package == ENTRY_PACKAGE_ID {
            continue;
        }
        let segments: Vec<&str> = package.split('/').filter(|s| !s.is_empty()).collect();
        let mut prefix = String::new();
        for (i, segment) in segments.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            if i + 1 == segments.len() {
                live.insert(prefix.clone());
            } else {
                ancestors.insert(prefix.clone());
            }
        }
    }
    for dir in &live {
        ancestors.remove(dir);
    }
    (live, ancestors)
}

/// Removes `.go` files (generated ones included) directly in `dir`, not recursing.
fn remove_direct_go_files(dir: &Path) -> io::Result<()> {
    let entries = match read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_file() && path.extension().is_some_and(|ext| ext == "go") {
            remove_file(&path)?;
        }
    }

    Ok(())
}

/// `remove_dir_all` that treats an already-absent directory as success.
fn remove_dir_all_if_present(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Removes interface caches in `target/.lisette/cache/` for packages no longer in the graph.
fn prune_orphan_caches(target_dir: &Path, live_packages: &[String]) -> io::Result<()> {
    let cache_dir = target_dir.join(".lisette").join("cache");
    let live: HashSet<String> = live_packages
        .iter()
        .filter(|id| id.as_str() != ENTRY_PACKAGE_ID)
        .map(|id| cache_file_name(id))
        .collect();

    let entries = match read_dir(&cache_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        if Path::new(&name)
            .extension()
            .is_some_and(|ext| ext == "cache")
            && !live.contains(name.to_string_lossy().as_ref())
        {
            remove_file(entry.path())?;
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
    use rayon::prelude::*;

    const PARALLEL_DIR_THRESHOLD: usize = 8;

    let Ok(entries) = read_dir(dir) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if entry_is_dir(&entry, &path) {
            subdirs.push(path);
        } else if path.extension().is_some_and(|e| e == "lis") {
            files.push(path);
        }
    }

    let nested: Vec<PathBuf> = if subdirs.len() < PARALLEL_DIR_THRESHOLD {
        subdirs
            .iter()
            .flat_map(|d| collect_lis_filepaths_recursive(d))
            .collect()
    } else {
        subdirs
            .par_iter()
            .flat_map_iter(|d| collect_lis_filepaths_recursive(d))
            .collect()
    };
    files.extend(nested);
    files
}

fn entry_is_dir(entry: &std::fs::DirEntry, path: &Path) -> bool {
    match entry.file_type() {
        Ok(file_type) if file_type.is_symlink() => false,
        Ok(file_type) => file_type.is_dir(),
        Err(_) => path.is_dir(),
    }
}

impl Loader for LocalFileSystem {
    fn scan_folder(&self, folder_name: &str) -> Files {
        if semantics::loader::is_external_test_package(folder_name)
            && let Some((project_root, display_base)) = &self.project_root
        {
            return self.collect_files(&project_root.join(folder_name), folder_name, display_base);
        }

        let fs_name = to_fs_path(folder_name);
        for (search_path, display_base) in &self.search_paths {
            let folder_path = if fs_name.is_empty() {
                search_path.clone()
            } else {
                search_path.join(fs_name)
            };
            let files = self.collect_files(&folder_path, fs_name, display_base);

            if !files.is_empty() {
                return files;
            }
        }

        HashMap::default()
    }

    fn discover_packages(&self) -> DiscoveredPackages {
        let Some((root, _)) = self.search_paths.first() else {
            return DiscoveredPackages::default();
        };
        #[derive(Default)]
        struct Contents {
            has_test: bool,
            has_test_root_file: bool,
            has_production: bool,
        }

        let mut contents_by_dir: HashMap<PathBuf, Contents> = HashMap::new();
        let walked;
        let sources = match &self.scanned_sources {
            Some(sources) => sources.as_slice(),
            None => {
                walked = collect_lis_filepaths_recursive(root);
                &walked
            }
        };
        for path in sources {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(dir) = path.parent() else {
                continue;
            };
            let contents = contents_by_dir.entry(dir.to_path_buf()).or_default();
            if name.ends_with(".test.lis") {
                contents.has_test = true;
            } else {
                contents.has_test_root_file = true;
                if semantics::loader::is_production_package_file(name) {
                    contents.has_production = true;
                }
            }
        }
        let dir_to_id = |dir: &Path| dir.strip_prefix(root).ok().map(package_id_from_rel);
        let mut discovered = DiscoveredPackages::default();
        for (dir, contents) in contents_by_dir {
            let Some(package_id) = dir_to_id(&dir) else {
                continue;
            };
            let has_internal_tests = contents.has_test && contents.has_test_root_file;
            if contents.has_production {
                discovered.add_production(package_id, has_internal_tests);
            } else if has_internal_tests {
                discovered.add_internal_test_root(package_id);
            }
        }
        for package_id in self.discover_external_test_roots() {
            discovered.add_external_test_root(package_id);
        }
        discovered
    }
}

fn package_id_from_rel(rel: &Path) -> String {
    let joined: Vec<&str> = rel
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    if joined.is_empty() {
        ENTRY_PACKAGE_ID.to_string()
    } else {
        joined.join("/")
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

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

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

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

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

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(tmp.path().join("wire_gen.go").exists());
    }

    #[test]
    fn prunes_file_without_marker() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "stale.go", "package main\n\nfunc Old() {}\n");
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

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

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn prunes_partial_marker_missing_do_not_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// Code generated by Wire.\n\npackage main\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn prunes_marker_with_wrong_casing() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// code generated by Wire. DO NOT EDIT.\n\npackage main\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn prunes_marker_with_collapsed_spaces() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "// Code generated DO NOT EDIT.\n\npackage main\n";
        write_file(tmp.path(), "fake.go", content);
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(!tmp.path().join("fake.go").exists());
    }

    #[test]
    fn produced_files_are_kept_regardless_of_marker() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(tmp.path().join("main.go").exists());
    }

    #[test]
    fn non_go_files_are_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "go.mod", "module foo\n");
        write_file(tmp.path(), "go.sum", "");
        write_file(tmp.path(), "main.go", "package main\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &[]).unwrap();

        assert!(tmp.path().join("go.mod").exists());
        assert!(tmp.path().join("go.sum").exists());
    }

    #[test]
    fn prunes_removed_package_directory() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("greet")).unwrap();
        write_file(&tmp.path().join("greet"), "greet.go", "package greet\n");

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "greet/greet.go"],
            &[],
        )
        .unwrap();

        assert!(!tmp.path().join("greet/greet.go").exists());
        assert!(!tmp.path().join("greet").exists());
        assert!(tmp.path().join("main.go").exists());
    }

    #[test]
    fn keeps_live_package_directory_even_when_not_produced() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("greet")).unwrap();
        write_file(&tmp.path().join("greet"), "greet.go", "package greet\n");

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "greet/greet.go"],
            &["greet".to_string()],
        )
        .unwrap();

        assert!(tmp.path().join("greet/greet.go").exists());
    }

    #[test]
    fn keeps_nested_live_package_and_prunes_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("deep/nested/mod")).unwrap();
        write_file(
            &tmp.path().join("deep/nested/mod"),
            "mod.go",
            "package mod\n",
        );
        stdfs::create_dir_all(tmp.path().join("deep/gone")).unwrap();
        write_file(&tmp.path().join("deep/gone"), "gone.go", "package gone\n");

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "deep/nested/mod/mod.go", "deep/gone/gone.go"],
            &["deep/nested/mod".to_string()],
        )
        .unwrap();

        assert!(tmp.path().join("deep/nested/mod/mod.go").exists());
        assert!(!tmp.path().join("deep/gone").exists());
    }

    #[test]
    fn prunes_orphan_parent_package_keeping_live_child() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("deep/nested/mod")).unwrap();
        write_file(&tmp.path().join("deep"), "deep.go", "package deep\n");
        write_file(
            &tmp.path().join("deep/nested/mod"),
            "mod.go",
            "package mod\n",
        );

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "deep/deep.go", "deep/nested/mod/mod.go"],
            &["deep/nested/mod".to_string()],
        )
        .unwrap();

        assert!(!tmp.path().join("deep/deep.go").exists());
        assert!(tmp.path().join("deep/nested/mod/mod.go").exists());
    }

    #[test]
    fn keeps_parent_package_that_is_also_live() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("deep/nested/mod")).unwrap();
        write_file(&tmp.path().join("deep"), "deep.go", "package deep\n");
        write_file(
            &tmp.path().join("deep/nested/mod"),
            "mod.go",
            "package mod\n",
        );

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "deep/deep.go", "deep/nested/mod/mod.go"],
            &["deep".to_string(), "deep/nested/mod".to_string()],
        )
        .unwrap();

        assert!(tmp.path().join("deep/deep.go").exists());
        assert!(tmp.path().join("deep/nested/mod/mod.go").exists());
    }

    #[test]
    fn leaves_directories_lisette_never_emitted_into() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("vendor/dep")).unwrap();
        write_file(&tmp.path().join("vendor/dep"), "dep.go", "package dep\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &["main.go"], &[]).unwrap();

        assert!(tmp.path().join("vendor/dep/dep.go").exists());
    }

    #[test]
    fn departed_package_dir_is_removed_with_its_generated_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("greet")).unwrap();
        write_file(&tmp.path().join("greet"), "greet.go", "package greet\n");
        write_file(
            &tmp.path().join("greet"),
            "wire_gen.go",
            "// Code generated by Wire. DO NOT EDIT.\n\npackage greet\n",
        );

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "greet/greet.go"],
            &[],
        )
        .unwrap();

        assert!(!tmp.path().join("greet").exists());
    }

    #[test]
    fn prunes_orphan_cache_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join(".lisette").join("cache");
        stdfs::create_dir_all(&cache).unwrap();
        write_file(&cache, "greet.cache", "");
        write_file(&cache, "kept.cache", "");

        prune_orphan_go_files(tmp.path(), &["main.go"], &[], &["kept".to_string()]).unwrap();

        assert!(!cache.join("greet.cache").exists());
        assert!(cache.join("kept.cache").exists());
    }

    #[test]
    fn leaves_lisette_working_state_dir_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join(".lisette")).unwrap();
        write_file(&tmp.path().join(".lisette"), "tool.go", "package lisette\n");

        prune_orphan_go_files(tmp.path(), &["main.go"], &["main.go"], &[]).unwrap();

        assert!(tmp.path().join(".lisette/tool.go").exists());
    }

    #[test]
    fn departed_cache_package_dir_is_removed_wholesale() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("cache")).unwrap();
        write_file(&tmp.path().join("cache"), "cache.go", "package cache\n");

        prune_orphan_go_files(
            tmp.path(),
            &["main.go"],
            &["main.go", "cache/cache.go"],
            &[],
        )
        .unwrap();

        assert!(
            !tmp.path().join("cache").exists(),
            "`cache` is an ordinary package dir now and departs wholesale"
        );
    }

    #[test]
    fn live_cache_package_go_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "main.go", "package main\n");
        stdfs::create_dir_all(tmp.path().join("cache")).unwrap();
        write_file(&tmp.path().join("cache"), "cache.go", "package cache\n");

        prune_orphan_go_files(
            tmp.path(),
            &["main.go", "cache/cache.go"],
            &["main.go", "cache/cache.go"],
            &["cache".to_string()],
        )
        .unwrap();

        assert!(tmp.path().join("cache/cache.go").exists());
    }

    #[test]
    fn collects_nested_lis_files_past_parallel_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_file(root, "main.lis", "fn main() {}\n");
        write_file(root, "ignore.go", "package main\n");
        let mut expected = vec![root.join("main.lis")];
        for i in 0..20 {
            let dir = root.join(format!("m{i}")).join("inner");
            stdfs::create_dir_all(&dir).unwrap();
            expected.push(write_file(&dir, &format!("m{i}.lis"), "pub fn x() {}\n"));
            write_file(&dir, "notes.txt", "x");
        }

        let mut found = collect_lis_filepaths_recursive(root);
        found.sort();
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn discover_packages_separates_production_and_test_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let with_both = root.join("alpha").join("beta");
        stdfs::create_dir_all(&with_both).unwrap();
        write_file(&with_both, "beta.lis", "pub fn x() {}\n");
        write_file(&with_both, "beta.test.lis", "#[test]\nfn t() {}\n");

        let test_only = root.join("gamma");
        stdfs::create_dir_all(&test_only).unwrap();
        write_file(&test_only, "gamma.test.lis", "#[test]\nfn t() {}\n");

        let prod_only = root.join("delta");
        stdfs::create_dir_all(&prod_only).unwrap();
        write_file(&prod_only, "delta.lis", "pub fn x() {}\n");

        let decl_only = root.join("epsilon");
        stdfs::create_dir_all(&decl_only).unwrap();
        write_file(&decl_only, "epsilon.d.lis", "pub fn ext() -> int\n");

        let fs = LocalFileSystem::new(root.to_str().unwrap(), None);
        let discovered = fs.discover_packages();

        let mut production = discovered
            .production_packages()
            .cloned()
            .collect::<Vec<_>>();
        production.sort();
        assert_eq!(
            production,
            vec!["alpha/beta".to_string(), "delta".to_string()],
            "production packages are non-test, non-declaration dirs"
        );

        let mut internal_test_roots = discovered
            .internal_test_roots()
            .cloned()
            .collect::<Vec<_>>();
        internal_test_roots.sort();
        assert_eq!(
            internal_test_roots,
            vec!["alpha/beta".to_string()],
            "a test root needs both a test file and a production file"
        );
    }

    #[test]
    fn discover_packages_finds_external_test_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        stdfs::create_dir_all(&src).unwrap();
        write_file(&src, "main.lis", "fn main() {}\n");

        let tests_root = tmp.path().join("tests");
        stdfs::create_dir_all(&tests_root).unwrap();
        write_file(&tests_root, "arithmetic.test.lis", "#[test]\nfn t() {}\n");

        let nested = tests_root.join("integration");
        stdfs::create_dir_all(&nested).unwrap();
        write_file(&nested, "flow.test.lis", "#[test]\nfn t() {}\n");

        let empty = tests_root.join("empty");
        stdfs::create_dir_all(&empty).unwrap();

        let fs = LocalFileSystem::new(src.to_str().unwrap(), Some(tmp.path()));
        let discovered = fs.discover_packages();
        let mut external = discovered
            .external_test_roots()
            .cloned()
            .collect::<Vec<_>>();
        external.sort();
        assert_eq!(
            external,
            vec!["tests".to_string(), "tests/integration".to_string()],
            "every tests/ dir holding a .test.lis is an external test package"
        );
    }

    #[test]
    fn scan_folder_roots_external_tests_at_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        stdfs::create_dir_all(src.join("tests")).unwrap();
        write_file(&src.join("tests"), "shadowed.lis", "pub fn x() {}\n");

        let tests_root = tmp.path().join("tests");
        stdfs::create_dir_all(&tests_root).unwrap();
        write_file(&tests_root, "arithmetic.test.lis", "#[test]\nfn t() {}\n");

        let fs = LocalFileSystem::new(src.to_str().unwrap(), Some(tmp.path()));
        let files = fs.scan_folder("tests");
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            vec!["arithmetic.test.lis"],
            "the tests namespace resolves against the project root, not src/"
        );
    }
}
