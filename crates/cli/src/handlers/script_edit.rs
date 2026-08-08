use std::path::{Path, PathBuf};

use super::script_table::ScriptTable;
use crate::cli_error;

pub(super) fn refuse_project_file(file: &Path, heading: &str) -> Result<(), i32> {
    let root = match super::project::resolve_file_target(file) {
        super::project::FileTarget::ProjectEntry { root }
        | super::project::FileTarget::ProjectPackage { root } => root,
        super::project::FileTarget::Script { .. } => return Ok(()),
    };
    cli_error!(
        heading,
        format!(
            "`{}` is a source file of the project at `{}`, which records dependencies in `lisette.toml`",
            file.display(),
            root.display()
        ),
        "Drop `--script` and run the command from the project root"
    );
    Err(1)
}

pub(super) fn save_unchanged(table: &ScriptTable, file: &Path, source: &str) -> Result<(), String> {
    let current = std::fs::read_to_string(file)
        .map_err(|e| format!("Failed to read `{}`: {}", file.display(), e))?;
    if current != source {
        return Err(format!(
            "`{}` changed while it was being resolved, so nothing was written",
            file.display()
        ));
    }
    table.save(file, source)
}

pub(super) fn read(file: &Path, heading: &str) -> Option<String> {
    match std::fs::read_to_string(file) {
        Ok(source) => Some(source),
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to read `{}`: {}", file.display(), e),
                "Check the path"
            );
            None
        }
    }
}

pub(super) fn script_dir(file: &Path, heading: &str) -> Result<PathBuf, i32> {
    let dir = super::script::script_build_dir(file);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        cli_error!(heading, e.to_string(), "Check permissions on the temp dir");
        return Err(1);
    }
    Ok(dir)
}

pub(super) fn write_go_mod(dir: &Path, table: &ScriptTable, heading: &str) -> Result<(), i32> {
    let locator = super::script_deps::locator(table.deps(), dir, super::script_deps::Mode::Offline);
    if let Err(message) = crate::go_cli::write_go_mod(dir, super::script::GO_MODULE, &locator) {
        cli_error!(heading, message, "Check permissions on the temp dir");
        return Err(1);
    }
    Ok(())
}

pub(super) fn third_party_imports(source: &str) -> Vec<String> {
    syntax::imports::scan_imports(source, 0)
        .into_iter()
        .filter_map(|import| {
            let package = import.name.strip_prefix("go:")?;
            (deps::is_third_party(package) && deps::is_valid_package_path(package))
                .then(|| package.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_third_party_imports_are_collected() {
        let source = "import \"go:fmt\"\nimport \"go:github.com/spf13/cobra\"\n";

        assert_eq!(third_party_imports(source), vec!["github.com/spf13/cobra"]);
    }
}
