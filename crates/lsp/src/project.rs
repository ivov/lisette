use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) enum ProjectConfig {
    Script(PathBuf),
    Workspace(PathBuf),
}

impl ProjectConfig {
    pub(crate) fn root(&self) -> &Path {
        match self {
            Self::Script(root) | Self::Workspace(root) => root,
        }
    }

    pub(crate) fn source_root(&self) -> PathBuf {
        match self {
            Self::Script(root) => root.clone(),
            Self::Workspace(root) => root.join("src"),
        }
    }

    pub(crate) fn is_script(&self) -> bool {
        matches!(self, Self::Script(_))
    }
}

pub(crate) fn find_project_root(start_path: &Path) -> Option<ProjectConfig> {
    let root = deps::find_project_root(start_path)?;
    let belongs = start_path.extension().is_some_and(|ext| ext == "lis")
        && start_path
            .strip_prefix(&root)
            .is_ok_and(|relative| relative.starts_with("src") || relative.starts_with("tests"));
    belongs.then_some(ProjectConfig::Workspace(root))
}

pub(crate) fn resolve_script_root(file_path: &Path) -> ProjectConfig {
    let root = file_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    ProjectConfig::Script(root)
}
