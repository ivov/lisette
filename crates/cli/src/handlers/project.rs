use std::ffi::OsStr;
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
    deps::find_project_root(&std::env::current_dir().ok()?)
}

/// The compilation unit a named `.lis` file belongs to.
pub(crate) enum FileTarget {
    /// One file alone, `inside_project` when a project sits above it.
    Standalone {
        inside_project: bool,
    },
    ProjectEntry {
        root: PathBuf,
    },
    ProjectModule {
        root: PathBuf,
    },
}

/// Resolves the unit `file`, which must exist, is compiled as.
pub(crate) fn resolve_file_target(file: &Path) -> FileTarget {
    if file.extension() != Some(OsStr::new("lis")) {
        return FileTarget::Standalone {
            inside_project: false,
        };
    }

    let absolute = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());

    let Some(root) = deps::find_project_root(&absolute) else {
        return FileTarget::Standalone {
            inside_project: false,
        };
    };

    let outside = FileTarget::Standalone {
        inside_project: true,
    };
    let Ok(relative) = absolute.strip_prefix(&root) else {
        return outside;
    };
    // The walk needs absolute paths to climb, but every message shows a root.
    let root = lisette::fs::relative_to_cwd(&root)
        .map(PathBuf::from)
        .unwrap_or(root);
    let mut components = relative.components();
    match components.next().and_then(|c| c.as_os_str().to_str()) {
        Some("src") => {
            if relative == Path::new("src/main.lis") {
                FileTarget::ProjectEntry { root }
            } else {
                FileTarget::ProjectModule { root }
            }
        }
        Some(semantics::loader::EXTERNAL_TESTS_DIR) => FileTarget::ProjectModule { root },
        _ => outside,
    }
}

pub(crate) fn project_label(root: &Path) -> String {
    deps::parse_manifest(root)
        .map(|manifest| manifest.project.name)
        .unwrap_or_else(|_| {
            lisette::fs::relative_to_cwd(root).unwrap_or_else(|| root.display().to_string())
        })
}
