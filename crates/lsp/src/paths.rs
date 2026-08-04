use std::path::{Path, PathBuf};

use crate::protocol::Url;
use semantics::loader::is_external_test_package;

use crate::project::ProjectConfig;

pub(crate) use syntax::ENTRY_PACKAGE_ID;

pub(crate) fn package_id_from_components(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn package_id_to_dir(config: &ProjectConfig, package_id: &str) -> PathBuf {
    if !config.is_script() && is_external_test_package(package_id) {
        return config.root().join(package_id);
    }
    let source_root = config.source_root();
    if package_id == ENTRY_PACKAGE_ID {
        source_root
    } else {
        source_root.join(package_id)
    }
}

pub(crate) fn package_file_to_path(
    config: &ProjectConfig,
    package_id: &str,
    filename: &str,
) -> PathBuf {
    package_id_to_dir(config, package_id).join(filename)
}

fn path_to_package_file(
    config: &ProjectConfig,
    file_path: &Path,
) -> Option<(String, String, bool)> {
    let filename = file_path.file_name()?.to_str()?.to_string();

    if !config.is_script()
        && let Ok(relative) = file_path.strip_prefix(config.root())
        && let Some(dir) = relative.parent()
    {
        let package_id = package_id_from_components(dir);
        if is_external_test_package(&package_id) {
            return Some((package_id, filename, true));
        }
    }

    let source_root = config.source_root();
    let relative = file_path.strip_prefix(&source_root).ok()?;
    let parent = relative.parent()?;
    let package_id = if parent.as_os_str().is_empty() {
        ENTRY_PACKAGE_ID.to_string()
    } else {
        parent.to_str()?.to_string()
    };
    Some((package_id, filename, false))
}

pub(crate) fn uri_to_package_file(
    config: &ProjectConfig,
    uri: &Url,
) -> Option<(String, String, bool)> {
    let file_path = uri.to_file_path().ok()?;
    path_to_package_file(config, &file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_source_root_maps_to_entry_package() {
        let config = ProjectConfig::Workspace(PathBuf::from("workspace"));

        assert_eq!(
            path_to_package_file(&config, Path::new("workspace/src/main.lis")),
            Some((ENTRY_PACKAGE_ID.to_string(), "main.lis".to_string(), false))
        );
    }

    #[test]
    fn workspace_subdirectory_maps_to_named_package() {
        let config = ProjectConfig::Workspace(PathBuf::from("workspace"));

        assert_eq!(
            path_to_package_file(&config, Path::new("workspace/src/math/vector.lis")),
            Some(("math".to_string(), "vector.lis".to_string(), false))
        );
    }

    #[test]
    fn script_subdirectory_maps_to_named_package() {
        let config = ProjectConfig::Script(PathBuf::from("script"));

        assert_eq!(
            path_to_package_file(&config, Path::new("script/math/vector.lis")),
            Some(("math".to_string(), "vector.lis".to_string(), false))
        );
    }
}
