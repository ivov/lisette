use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::Url;

use crate::project::ProjectConfig;

pub(crate) const ENTRY_MODULE_ID: &str = "_entry_";

pub(crate) fn module_id_to_dir(config: &ProjectConfig, module_id: &str) -> PathBuf {
    let source_root = config.source_root();
    if module_id == ENTRY_MODULE_ID {
        source_root
    } else {
        source_root.join(module_id)
    }
}

pub(crate) fn module_file_to_path(
    config: &ProjectConfig,
    module_id: &str,
    filename: &str,
) -> PathBuf {
    module_id_to_dir(config, module_id).join(filename)
}

fn path_to_module_file(config: &ProjectConfig, file_path: &Path) -> Option<(String, String)> {
    let filename = file_path.file_name()?.to_str()?.to_string();

    let source_root = config.source_root();
    let relative = file_path.strip_prefix(&source_root).ok()?;
    let parent = relative.parent()?;
    let module_id = if parent.as_os_str().is_empty() {
        ENTRY_MODULE_ID.to_string()
    } else {
        parent.to_str()?.to_string()
    };
    Some((module_id, filename))
}

pub(crate) fn uri_to_module_file(config: &ProjectConfig, uri: &Url) -> Option<(String, String)> {
    let file_path = uri.to_file_path().ok()?;
    path_to_module_file(config, &file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_source_root_maps_to_entry_module() {
        let config = ProjectConfig::Workspace(PathBuf::from("workspace"));

        assert_eq!(
            path_to_module_file(&config, Path::new("workspace/src/main.lis")),
            Some((ENTRY_MODULE_ID.to_string(), "main.lis".to_string()))
        );
    }

    #[test]
    fn workspace_subdirectory_maps_to_named_module() {
        let config = ProjectConfig::Workspace(PathBuf::from("workspace"));

        assert_eq!(
            path_to_module_file(&config, Path::new("workspace/src/math/vector.lis")),
            Some(("math".to_string(), "vector.lis".to_string()))
        );
    }

    #[test]
    fn standalone_subdirectory_maps_to_named_module() {
        let config = ProjectConfig::Standalone(PathBuf::from("standalone"));

        assert_eq!(
            path_to_module_file(&config, Path::new("standalone/math/vector.lis")),
            Some(("math".to_string(), "vector.lis".to_string()))
        );
    }
}
