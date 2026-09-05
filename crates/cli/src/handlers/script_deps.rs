//! A script's `[dependencies.go]` table as the map a `TypedefLocator` wants.

use std::collections::BTreeMap;
use std::path::Path;

use crate::workspace::WorkspaceBindgen;
use deps::{GoDependency, TypedefLocator};
use std::path::PathBuf;
use std::sync::Arc;
use stdlib::Target;
use syntax::dependency_block;
use syntax::dependency_block::DependencyBlock;

/// Whether resolution may reach the network.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// `check` and the editor.
    Offline,
    /// `run` and `build`.
    Online,
}

pub(crate) fn script_deps(source: &str) -> BTreeMap<String, GoDependency> {
    let Some(block) = deps_block(source) else {
        return BTreeMap::new();
    };
    let Ok(table) = deps::parse_dependency_table(&block.text) else {
        return BTreeMap::new();
    };
    table
        .into_dependencies()
        .into_iter()
        .filter(|(module_path, dep)| deps::validate_script_entry(module_path, dep).is_ok())
        .collect()
}

pub(crate) fn deps_block(source: &str) -> Option<DependencyBlock> {
    dependency_block::scan_dependency_blocks(source, 0)
        .into_iter()
        .next()
}

pub(crate) fn locator(
    deps: BTreeMap<String, GoDependency>,
    dir: &Path,
    mode: Mode,
    target: Target,
) -> TypedefLocator {
    let locator = TypedefLocator::new(deps, Some(dir.to_path_buf()), target);
    if mode == Mode::Offline || locator.deps().is_empty() {
        return locator;
    }

    locator.with_bindgen(Arc::new(WorkspaceBindgen::new(
        dir.to_path_buf(),
        typedef_dir(dir),
        target,
    )))
}

pub(crate) fn typedef_dir(dir: &Path) -> PathBuf {
    deps::typedef_cache_dir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "// [dependencies.go]\n// \"golang.org/x/text\" = \"v0.21.0\"\n\nimport \"go:golang.org/x/text/cases\"\n";

    #[test]
    fn one_entry_covers_every_package_in_its_module() {
        let deps = script_deps(TABLE);

        assert_eq!(deps.len(), 1);
        assert!(deps.contains_key("golang.org/x/text"));
    }

    #[test]
    fn transitive_entries_are_ordinary_rows() {
        let source = "// [dependencies.go]\n// \"github.com/spf13/cobra\" = \"v1.8.1\"\n// \"github.com/spf13/pflag\" = { version = \"v1.0.5\", via = [\"github.com/spf13/cobra\"] }\n";
        let deps = script_deps(source);

        assert_eq!(deps.len(), 2);
        assert!(deps.contains_key("github.com/spf13/pflag"));
    }

    #[test]
    fn a_script_with_no_table_has_no_dependencies() {
        assert!(script_deps("import \"go:fmt\"\n").is_empty());
    }

    #[test]
    fn an_invalid_entry_is_dropped_rather_than_guessed() {
        let source = "// [dependencies.go]\n// \"github.com/google/uuid\" = \"latest\"\n";

        assert!(script_deps(source).is_empty());
    }
}
