use std::collections::HashMap;
use std::hash::BuildHasher;

use ecow::EcoString;

use crate::ast::{Expression, ImportAlias, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub id: u32,
    pub module_id: String,
    /// Stable bare filename (e.g. `greet.lis`); identity key for caching and
    /// LSP path reconstruction.
    pub name: String,
    /// Cwd-relative path for diagnostics and `--sourcemap` directives; equals
    /// `name` for synthetic/test loaders that have no notion of cwd.
    pub display_path: String,
    pub source: String,
    pub items: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileImport {
    pub name: EcoString,
    pub name_span: Span,
    pub alias: Option<ImportAlias>,
    pub span: Span,
}

impl FileImport {
    pub fn effective_alias<S: BuildHasher>(
        &self,
        go_package_names: &HashMap<String, String, S>,
    ) -> Option<String> {
        match &self.alias {
            Some(ImportAlias::Named(name, _)) => Some(name.to_string()),
            Some(ImportAlias::Blank(_)) => None,
            None => {
                if let Some(pkg_name) = go_package_names.get(self.name.as_str()) {
                    return Some(pkg_name.clone());
                }
                let default = match self.name.strip_prefix("go:") {
                    Some(go_path) => go_import_default_name(go_path),
                    None => self.name.rsplit('/').next().unwrap_or(&self.name),
                };
                Some(default.to_string())
            }
        }
    }
}

pub fn go_import_default_name(import_path: &str) -> &str {
    let path = import_path.strip_prefix("go:").unwrap_or(import_path);
    let mut segments = path.rsplit('/');
    let last = segments.next().unwrap_or(path);
    if is_major_version_segment(last)
        && let Some(preceding) = segments.next()
    {
        return preceding;
    }
    last
}

fn is_major_version_segment(segment: &str) -> bool {
    segment
        .strip_prefix('v')
        .and_then(|digits| digits.parse::<u32>().ok())
        .is_some_and(|major| major >= 2)
}

impl File {
    pub fn new(
        module_id: &str,
        name: &str,
        display_path: &str,
        source: &str,
        items: Vec<Expression>,
        id: u32,
    ) -> Self {
        File {
            id,
            module_id: module_id.to_string(),
            name: name.to_string(),
            display_path: display_path.to_string(),
            source: source.to_string(),
            items,
        }
    }

    pub fn new_cached(
        module_id: &str,
        name: &str,
        display_path: &str,
        source: &str,
        id: u32,
    ) -> Self {
        Self {
            id,
            module_id: module_id.to_string(),
            name: name.to_string(),
            display_path: display_path.to_string(),
            source: source.to_string(),
            items: vec![],
        }
    }

    pub fn is_d_lis(&self) -> bool {
        self.name.ends_with(".d.lis")
    }

    pub fn is_lis(&self) -> bool {
        !self.is_d_lis()
    }

    /// A test file (`*.test.lis`).
    pub fn is_test(&self) -> bool {
        self.name.ends_with(".test.lis")
    }

    pub fn imports(&self) -> Vec<FileImport> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Expression::ModuleImport {
                    name,
                    name_span,
                    alias,
                    span,
                } => Some(FileImport {
                    name: name.clone(),
                    name_span: *name_span,
                    alias: alias.clone(),
                    span: *span,
                }),
                _ => None,
            })
            .collect()
    }

    pub fn go_filename(&self) -> String {
        if let Some(stem) = self.name.strip_suffix(".test.lis") {
            return format!("{stem}_test.go");
        }
        std::path::Path::new(&self.name)
            .with_extension("go")
            .display()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::go_import_default_name;

    #[test]
    fn major_version_suffix_resolves_to_preceding_segment() {
        assert_eq!(go_import_default_name("github.com/pion/sdp/v3"), "sdp");
        assert_eq!(
            go_import_default_name("go:github.com/pion/webrtc/v4"),
            "webrtc"
        );
        assert_eq!(
            go_import_default_name("go:github.com/pion/transport/v4"),
            "transport"
        );
    }

    #[test]
    fn non_version_last_segment_is_kept() {
        assert_eq!(go_import_default_name("go:strings"), "strings");
        assert_eq!(
            go_import_default_name("go:github.com/pion/datachannel"),
            "datachannel"
        );
        assert_eq!(
            go_import_default_name("go:github.com/pion/transport/v4/packetio"),
            "packetio"
        );
    }

    #[test]
    fn v0_and_v1_are_ordinary_segments() {
        assert_eq!(go_import_default_name("go:k8s.io/api/core/v1"), "v1");
        assert_eq!(go_import_default_name("go:example.com/pkg/v0"), "v0");
    }

    #[test]
    fn dotted_version_suffix_is_not_a_major_version_segment() {
        assert_eq!(go_import_default_name("go:gopkg.in/yaml.v3"), "yaml.v3");
    }

    #[test]
    fn version_like_segment_without_preceding_segment_is_kept() {
        assert_eq!(go_import_default_name("v2"), "v2");
        assert_eq!(go_import_default_name("go:v2"), "v2");
    }

    #[test]
    fn bare_v_or_non_numeric_is_not_a_version() {
        assert_eq!(go_import_default_name("go:example.com/foo/v"), "v");
        assert_eq!(go_import_default_name("go:example.com/foo/vx"), "vx");
    }
}
