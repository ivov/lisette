use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) enum ProjectConfig {
    Standalone(PathBuf),
    Workspace(PathBuf),
}

impl ProjectConfig {
    pub(crate) fn root(&self) -> &Path {
        match self {
            Self::Standalone(root) | Self::Workspace(root) => root,
        }
    }

    pub(crate) fn source_root(&self) -> PathBuf {
        match self {
            Self::Standalone(root) => root.clone(),
            Self::Workspace(root) => root.join("src"),
        }
    }

    pub(crate) fn is_standalone(&self) -> bool {
        matches!(self, Self::Standalone(_))
    }
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
            return Some(ProjectConfig::Workspace(current));
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

    ProjectConfig::Standalone(root)
}
