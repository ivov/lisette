use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::Url;

use crate::project::ProjectConfig;

pub(crate) const ENTRY_MODULE_ID: &str = "_entry_";

pub(crate) fn module_id_to_dir(config: &ProjectConfig, module_id: &str) -> PathBuf {
    if config.standalone_mode {
        if module_id == ENTRY_MODULE_ID {
            config.root.clone()
        } else {
            config.root.join(module_id)
        }
    } else if module_id == ENTRY_MODULE_ID {
        config.root.join("src")
    } else {
        config.root.join("src").join(module_id)
    }
}

pub(crate) fn module_file_to_path(
    config: &ProjectConfig,
    module_id: &str,
    filename: &str,
) -> PathBuf {
    if config.standalone_mode {
        if module_id == ENTRY_MODULE_ID {
            config.root.join(filename)
        } else {
            config.root.join(module_id).join(filename)
        }
    } else {
        let module_dir = if module_id == ENTRY_MODULE_ID {
            config.root.join("src")
        } else {
            config.root.join("src").join(module_id)
        };
        module_dir.join(filename)
    }
}

fn path_to_module_file(config: &ProjectConfig, file_path: &Path) -> Option<(String, String)> {
    let filename = file_path.file_name()?.to_str()?.to_string();

    if config.standalone_mode {
        if file_path.parent()? == config.root {
            return Some((ENTRY_MODULE_ID.to_string(), filename));
        }
        let relative = file_path.strip_prefix(&config.root).ok()?;
        let module_id = relative.parent()?.to_str()?.to_string();
        Some((module_id, filename))
    } else {
        let src_dir = config.root.join("src");
        let relative = file_path.strip_prefix(&src_dir).ok()?;

        let module_id = if relative
            .parent()
            .map(|p| p.as_os_str().is_empty())
            .unwrap_or(true)
        {
            ENTRY_MODULE_ID.to_string()
        } else {
            relative.parent()?.to_str()?.to_string()
        };

        Some((module_id, filename))
    }
}

pub(crate) fn uri_to_module_file(config: &ProjectConfig, uri: &Url) -> Option<(String, String)> {
    let file_path = uri.to_file_path().ok()?;
    path_to_module_file(config, &file_path)
}
