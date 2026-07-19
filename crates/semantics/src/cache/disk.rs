use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Serialize, de::DeserializeOwned};

pub(super) fn global_path(file_name: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".lisette")
            .join("cache")
            .join(file_name),
    )
}

pub(super) fn read<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    bincode::deserialize(&bytes).map_err(|error| {
        let _ = fs::remove_file(path);
        io::Error::new(io::ErrorKind::InvalidData, error)
    })
}

pub(super) fn write<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = bincode::serialize(value).map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = temp_path(path);
    let result = fs::write(&temp_path, bytes).and_then(|()| fs::rename(&temp_path, path));
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result
}

pub(super) fn write_global<T: Serialize>(path: &Path, value: &T, legacy_prefix: &str) {
    if write(path, value).is_ok()
        && let Some(parent) = path.parent()
    {
        prune_legacy(parent, legacy_prefix);
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path(final_path: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    final_path.with_extension(format!("tmp.{}.{}", std::process::id(), counter))
}

fn prune_legacy(dir: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.contains("_compiler_") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_round_trips_written_value() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache.bin");
        let expected = vec![1_u32, 2, 3];

        write(&path, &expected).unwrap();
        let actual: Vec<u32> = read(&path).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn write_creates_parent_directories() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join("cache.bin");

        write(&path, &42_u32).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn read_removes_corrupt_cache() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache.bin");
        fs::write(&path, b"invalid").unwrap();

        let error = read::<Vec<String>>(&path).unwrap_err();

        assert_eq!(
            (error.kind(), path.exists()),
            (io::ErrorKind::InvalidData, false)
        );
    }

    #[test]
    fn prune_legacy_removes_only_hashed_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let legacy_prelude = dir.join("prelude_defs_4330e9_compiler_f709f8.bin");
        let legacy_stdlib = dir.join("stdlib_defs_151b6b_compiler_f709f8_darwin_arm64.bin");
        let stable_prelude = dir.join("prelude_defs.bin");
        let stable_stdlib = dir.join("stdlib_defs_darwin_arm64.bin");
        let other_stdlib = dir.join("stdlib_defs_linux_amd64.bin");
        for path in [
            &legacy_prelude,
            &legacy_stdlib,
            &stable_prelude,
            &stable_stdlib,
            &other_stdlib,
        ] {
            fs::write(path, b"x").unwrap();
        }

        prune_legacy(dir, "prelude_defs");
        prune_legacy(dir, "stdlib_defs");

        assert_eq!(
            [
                legacy_prelude.exists(),
                legacy_stdlib.exists(),
                stable_prelude.exists(),
                stable_stdlib.exists(),
                other_stdlib.exists(),
            ],
            [false, false, true, true, true],
        );
    }

    #[test]
    fn prune_legacy_missing_dir_is_noop() {
        let temp = tempfile::tempdir().unwrap();
        prune_legacy(&temp.path().join("does_not_exist"), "prelude_defs");
    }

    #[test]
    fn temp_paths_are_unique() {
        let base = Path::new("/cache/prelude_defs.bin");
        let first = temp_path(base);
        let second = temp_path(base);
        assert_ne!(first, second);
    }
}
