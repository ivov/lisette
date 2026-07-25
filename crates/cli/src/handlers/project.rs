use std::fs::File;
use std::path::{Path, PathBuf};

use crate::lock::{acquire_mutation_lock, acquire_target_lock};
use crate::{cli_error, error};

pub(crate) struct MutationProject {
    pub(crate) root: PathBuf,
    pub(crate) target_dir: PathBuf,
    pub(crate) manifest: deps::Manifest,
    pub(crate) typedef_cache_dir: PathBuf,
    _mutation_lock: File,
    _target_lock: File,
}

impl MutationProject {
    pub(crate) fn open() -> Result<Self, i32> {
        let root = find_project_root().ok_or_else(|| {
            cli_error!(
                "No project found",
                "No `lisette.toml` in current directory or in any parent",
                "Run `lis new <name>` to create a project"
            );
            1
        })?;

        let manifest = deps::parse_manifest(&root).map_err(|message| {
            cli_error!("Failed to read manifest", message, "Fix `lisette.toml`");
            1
        })?;

        validate_manifest(&manifest)?;

        let target_dir = root.join("target");
        if target_dir.is_file() {
            cli_error!(
                "Failed to set up target directory",
                "`target/` exists but is a file, not a directory",
                "Remove or move `target/` and retry"
            );
            return Err(1);
        }
        std::fs::create_dir_all(&target_dir).map_err(|error| {
            error!(
                "failed to set up target directory",
                format!("Failed to create target directory: {error}")
            );
            1
        })?;

        let mutation_lock = acquire_mutation_lock(&target_dir)?;
        let target_lock = acquire_target_lock(&target_dir)?;
        let typedef_cache_dir = deps::typedef_cache_dir(&root);

        Ok(Self {
            root,
            target_dir,
            manifest,
            typedef_cache_dir,
            _mutation_lock: mutation_lock,
            _target_lock: target_lock,
        })
    }
}

fn validate_manifest(manifest: &deps::Manifest) -> Result<(), i32> {
    if let Err(message) = deps::check_toolchain_version(manifest) {
        let message = message
            .strip_prefix("Toolchain mismatch: ")
            .unwrap_or(&message)
            .to_string();
        error!("toolchain mismatch", message);
        return Err(1);
    }

    if let Err(message) = deps::check_no_subpackage_deps(manifest) {
        cli_error!(
            "Invalid `lisette.toml`",
            message,
            "Fix `lisette.toml` and retry"
        );
        return Err(1);
    }

    if let Err(message) = deps::validate_project_name(&manifest.project.name) {
        cli_error!(
            "Invalid project name",
            message,
            "Rename `project.name` in `lisette.toml`"
        );
        return Err(1);
    }

    Ok(())
}

fn find_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut current: &Path = &cwd;
    loop {
        if current.join("lisette.toml").is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}
