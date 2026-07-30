use std::fs::{self, File, TryLockError};
use std::path::Path;

use crate::{cli_error, error};

/// Project-scoped, fail-fast lock guarding `lisette.toml` mutations
pub fn acquire_mutation_lock(target_dir: &Path) -> Result<File, i32> {
    let lock_path = target_dir.join(".lis-mutate.lock");
    let file = create_lock_file(&lock_path)?;

    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            cli_error!(
                "Another manifest mutation is in progress",
                "A concurrent `lis add` or `lis sync` holds the project lock",
                "Wait for the other invocation to finish, then retry"
            );
            return Err(1);
        }
        Err(TryLockError::Error(err)) => {
            error!("failed to acquire lock", err.to_string());
            return Err(1);
        }
    }

    Ok(file)
}

/// Project-scoped, blocking lock guarding `target/` mutations and the typedef cache.
pub fn acquire_target_lock(target_dir: &Path) -> Result<File, i32> {
    target_lock_inner(target_dir).map_err(|msg| {
        error!("failed to acquire target lock", msg);
        1
    })
}

/// `acquire_target_lock` variant that returns the error as a `String`
/// for the LSP, which surfaces errors as analysis diagnostics.
pub(crate) fn acquire_target_lock_quiet(target_dir: &Path) -> Result<File, String> {
    target_lock_inner(target_dir)
}

fn target_lock_inner(target_dir: &Path) -> Result<File, String> {
    let dir = target_dir.join(".lisette");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create `{}`: {}", dir.display(), e))?;

    let lock_path = dir.join(".lis-target.lock");
    let file = File::create(&lock_path)
        .map_err(|e| format!("Failed to create `{}`: {}", lock_path.display(), e))?;

    file.lock()
        .map_err(|e| format!("Failed to acquire target lock: {}", e))?;

    Ok(file)
}

fn create_lock_file(path: &Path) -> Result<File, i32> {
    File::create(path).map_err(|e| {
        error!(
            "failed to create lock file",
            format!("Failed to create `{}`: {}", path.display(), e)
        );
        1
    })
}
