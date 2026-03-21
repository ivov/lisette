use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ProjectConfig {
    pub(crate) root: PathBuf,
    pub(crate) standalone_mode: bool,
}

pub(crate) fn find_project_root(start_path: &Path) -> Option<ProjectConfig> {
    let mut current = if start_path.is_file() {
        start_path.parent()?.to_path_buf()
    } else {
        start_path.to_path_buf()
    };

    loop {
        let manifest = current.join("lisette.toml");
        if manifest.exists() {
            return Some(ProjectConfig {
                root: current,
                standalone_mode: false,
            });
        }

        if !current.pop() {
            break;
        }
    }

    None
}

pub(crate) fn resolve_standalone_root(file_path: &Path) -> ProjectConfig {
    let root = file_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    ProjectConfig {
        root,
        standalone_mode: true,
    }
}
