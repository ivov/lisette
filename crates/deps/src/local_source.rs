//! Change detection for local Go modules, whose typedefs cannot be
//! content-addressed by version like every other dependency.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::project_manifest::{GoDependency, ReplacementSource};

/// Content, never mtime: a `git checkout` can move mtimes backwards.
pub(crate) fn stamp_hash(
    deps: &BTreeMap<String, GoDependency>,
    local_source: Option<&Path>,
) -> String {
    let mut hash = Fnv::new();
    for (module, dep) in deps {
        hash.write(module.as_bytes());
        match dep {
            GoDependency::Remote { version, .. } => hash.write(version.as_bytes()),
            GoDependency::Replaced {
                source: ReplacementSource::Module { path, version },
                ..
            } => {
                hash.write(path.as_bytes());
                hash.write(version.as_bytes());
            }
            GoDependency::Replaced {
                source: ReplacementSource::Local { path },
                ..
            } => {
                hash.write(path.as_bytes());
                if let Some(project_root) = local_source {
                    hash_local_module(&mut hash, &project_root.join(path));
                }
            }
        }
    }
    format!("{:016x}", hash.finish())
}

/// Stops at nested `go.mod` boundaries: a nested module is a separate module,
/// hashed by its own entry if declared.
fn hash_local_module(hash: &mut Fnv, module_dir: &Path) {
    match std::fs::read(module_dir.join("go.mod")) {
        Ok(bytes) => {
            hash.write(b"go.mod");
            hash.write(&bytes);
        }
        Err(_) => {
            hash.write(b"missing go.mod");
            return;
        }
    }
    let mut files = Vec::new();
    collect_go_files(module_dir, PathBuf::new(), &mut files);
    files.sort();
    for relative in files {
        hash.write(relative.as_os_str().as_encoded_bytes());
        if let Ok(bytes) = std::fs::read(module_dir.join(&relative)) {
            hash.write(&bytes);
        }
    }
}

fn collect_go_files(dir: &Path, relative: PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if path.join("go.mod").exists() {
                continue;
            }
            collect_go_files(&path, relative.join(&name), out);
        } else if name_str.ends_with(".go")
            && !name_str.ends_with("_test.go")
            // Regular files only: reading a FIFO named `x.go` blocks forever.
            && path.is_file()
        {
            out.push(relative.join(&name));
        }
    }
}

/// The stamp sidecar for a local typedef: `X.d.lis` -> `X.stamp`.
fn stamp_path_for_typedef(typedef_path: &Path) -> PathBuf {
    let name = typedef_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let stem = name.strip_suffix(".d.lis").unwrap_or(name);
    typedef_path.with_file_name(format!("{}.stamp", stem))
}

pub(crate) fn local_typedef_is_fresh(typedef_path: &Path, stamp: &str) -> bool {
    std::fs::read_to_string(stamp_path_for_typedef(typedef_path))
        .is_ok_and(|existing| existing == stamp)
}

/// On stamp mismatch, delete the typedef so resolution reroutes into the
/// regenerate-on-miss path. A failed eviction (e.g. a file lock on Windows) is
/// returned so the caller refuses the stale read, and keeps the old stamp so
/// eviction retries next time.
pub(crate) fn gate_local_typedef(typedef_path: &Path, stamp: &str) -> Result<(), std::io::Error> {
    if local_typedef_is_fresh(typedef_path, stamp) {
        return Ok(());
    }
    let stamp_path = stamp_path_for_typedef(typedef_path);
    match std::fs::remove_file(typedef_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Some(parent) = stamp_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&stamp_path, stamp);
    Ok(())
}

/// FNV-1a, matching the other deterministic hashes in the workspace.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x100_0000_01b3);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_dep(path: &str) -> GoDependency {
        GoDependency::Replaced {
            source: ReplacementSource::Local {
                path: path.to_string(),
            },
            via: None,
        }
    }

    fn deps_with(entries: Vec<(&str, GoDependency)>) -> BTreeMap<String, GoDependency> {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn hash_changes_on_go_source_edit_and_ignores_tests_and_mtime() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        write(root, "foo/go.mod", "module example.com/me/foo\n");
        write(root, "foo/foo.go", "package foo\n");
        write(root, "foo/foo_test.go", "package foo\n");
        let deps = deps_with(vec![("example.com/me/foo", local_dep("foo"))]);

        let before = stamp_hash(&deps, Some(root));

        write(root, "foo/foo_test.go", "package foo // edited\n");
        assert_eq!(before, stamp_hash(&deps, Some(root)), "test files ignored");

        write(root, "foo/foo.go", "package foo // edited\n");
        assert_ne!(
            before,
            stamp_hash(&deps, Some(root)),
            "source edits detected"
        );
    }

    #[test]
    fn hash_changes_on_go_mod_edit_and_on_remote_version_bump() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        write(root, "foo/go.mod", "module example.com/me/foo\n");
        write(root, "foo/foo.go", "package foo\n");

        let remote = |version: &str| GoDependency::Remote {
            version: version.to_string(),
            via: None,
        };
        let deps = deps_with(vec![
            ("example.com/me/foo", local_dep("foo")),
            ("github.com/google/uuid", remote("v1.6.0")),
        ]);
        let before = stamp_hash(&deps, Some(root));

        let bumped = deps_with(vec![
            ("example.com/me/foo", local_dep("foo")),
            ("github.com/google/uuid", remote("v1.7.0")),
        ]);
        assert_ne!(before, stamp_hash(&bumped, Some(root)));

        write(root, "foo/go.mod", "module example.com/me/foo\n\ngo 1.25\n");
        assert_ne!(before, stamp_hash(&deps, Some(root)));
    }

    #[test]
    fn hash_stops_at_nested_module_boundary() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        write(root, "foo/go.mod", "module example.com/me/foo\n");
        write(root, "foo/foo.go", "package foo\n");
        write(
            root,
            "foo/child/go.mod",
            "module example.com/me/foo/child\n",
        );
        write(root, "foo/child/child.go", "package child\n");
        let deps = deps_with(vec![("example.com/me/foo", local_dep("foo"))]);

        let before = stamp_hash(&deps, Some(root));
        write(root, "foo/child/child.go", "package child // edited\n");
        assert_eq!(before, stamp_hash(&deps, Some(root)));
    }

    #[test]
    fn gate_deletes_typedef_on_mismatch_and_keeps_it_on_match() {
        let dir = tempfile::tempdir().unwrap();
        let typedef = dir.path().join("foo.d.lis");
        std::fs::write(&typedef, "typedef").unwrap();

        gate_local_typedef(&typedef, "aaaa").unwrap();
        assert!(!typedef.exists(), "no stamp -> typedef discarded");
        assert_eq!(
            std::fs::read_to_string(stamp_path_for_typedef(&typedef)).unwrap(),
            "aaaa"
        );

        std::fs::write(&typedef, "typedef").unwrap();
        gate_local_typedef(&typedef, "aaaa").unwrap();
        assert!(typedef.exists(), "matching stamp -> typedef kept");

        gate_local_typedef(&typedef, "bbbb").unwrap();
        assert!(!typedef.exists(), "changed stamp -> typedef discarded");
    }

    #[test]
    fn stamp_path_swaps_the_full_d_lis_suffix() {
        assert_eq!(
            stamp_path_for_typedef(Path::new("/cache/local/mux.d.lis")),
            Path::new("/cache/local/mux.stamp")
        );
    }

    #[cfg(unix)]
    #[test]
    fn gate_keeps_old_stamp_when_eviction_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        let typedef = locked.join("foo.d.lis");
        std::fs::write(&typedef, "typedef").unwrap();
        std::fs::write(stamp_path_for_typedef(&typedef), "old").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

        let outcome = gate_local_typedef(&typedef, "new");

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            outcome.is_err(),
            "failed eviction is reported to the caller"
        );
        assert!(typedef.exists(), "typedef untouched when eviction fails");
        assert_eq!(
            std::fs::read_to_string(stamp_path_for_typedef(&typedef)).unwrap(),
            "old",
            "stale content is not blessed with the new stamp"
        );
    }
}
