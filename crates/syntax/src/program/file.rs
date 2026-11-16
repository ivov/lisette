use std::collections::HashMap;
use std::hash::BuildHasher;
use std::path::PathBuf;

use ecow::{EcoString, eco_format};

use crate::FileParseStatus;
use crate::ast::{Expression, ImportAlias, Span};
use crate::parse::{ParseError, ParseResult};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub id: u32,
    pub package_id: String,
    pub parse_status: FileParseStatus,
    /// Stable bare filename (e.g. `greet.lis`); identity key for caching and
    /// LSP path reconstruction.
    pub name: String,
    /// Cwd-relative path for diagnostics and `--sourcemap` directives, absolute
    /// outside the cwd. Equals `name` for loaders that have no notion of cwd.
    pub display_path: String,
    /// Physical source path when it cannot be reconstructed from the package
    /// and filename, notably for generated Go typedefs.
    pub source_path: Option<PathBuf>,
    pub source: String,
    pub items: Vec<Expression>,
    pub file_comment: Option<String>,
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
            None => Some(unaliased_binding_name(&self.name, go_package_names).to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFileKind {
    Production,
    Test,
    MisnamedTest,
    Typedef,
    Other,
}

impl SourceFileKind {
    pub fn of(name: &str) -> Self {
        if !name.ends_with(".lis") {
            Self::Other
        } else if name.ends_with(".d.lis") {
            Self::Typedef
        } else if name.ends_with(".test.lis") {
            Self::Test
        } else if name.ends_with("_test.lis") {
            Self::MisnamedTest
        } else {
            Self::Production
        }
    }

    pub fn is_lis(self) -> bool {
        !matches!(self, Self::Other)
    }

    pub fn emits_go(self) -> bool {
        match self {
            Self::Production | Self::MisnamedTest => true,
            Self::Test | Self::Typedef | Self::Other => false,
        }
    }
}

pub fn is_test_file(name: &str) -> bool {
    SourceFileKind::of(name) == SourceFileKind::Test
}

pub fn unaliased_binding_name<'a, S: BuildHasher>(
    path: &'a str,
    go_package_names: &'a HashMap<String, String, S>,
) -> &'a str {
    if let Some(package_name) = go_package_names.get(path) {
        return package_name;
    }
    if path.starts_with("go:") {
        return go_import_default_name(path);
    }
    path.rsplit('/').next().unwrap_or(path)
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
    /// Keep the parsed AST, status, and file comment together while returning its diagnostics.
    pub fn from_parse(
        id: u32,
        package_id: &str,
        name: String,
        display_path: String,
        source: String,
        result: ParseResult,
    ) -> (Self, Vec<ParseError>) {
        let file = Self {
            id,
            package_id: package_id.to_string(),
            name,
            display_path,
            source_path: None,
            source,
            parse_status: result.status,
            items: result.ast,
            file_comment: result.file_comment,
        };
        (file, result.errors)
    }

    pub fn new_cached(
        package_id: &str,
        name: &str,
        display_path: &str,
        source: &str,
        id: u32,
    ) -> Self {
        Self {
            id,
            package_id: package_id.to_string(),
            parse_status: FileParseStatus::Clean,
            name: name.to_string(),
            display_path: display_path.to_string(),
            source_path: None,
            source: source.to_string(),
            items: vec![],
            file_comment: None,
        }
    }

    pub fn is_d_lis(&self) -> bool {
        SourceFileKind::of(&self.name) == SourceFileKind::Typedef
    }

    /// A test file (`*.test.lis`).
    pub fn is_test(&self) -> bool {
        is_test_file(&self.name)
    }

    pub fn imports(&self) -> Vec<FileImport> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Expression::PackageImport {
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

    pub fn public_declarations(&self) -> Vec<EcoString> {
        let exports_all = self.is_d_lis();
        let mut names = Vec::new();
        for item in &self.items {
            let (name, visibility) = match item {
                Expression::Function {
                    name, visibility, ..
                }
                | Expression::Struct {
                    name, visibility, ..
                }
                | Expression::TypeAlias {
                    name, visibility, ..
                }
                | Expression::Interface {
                    name, visibility, ..
                }
                | Expression::VariableDeclaration {
                    name, visibility, ..
                } => (name, visibility),
                Expression::Const {
                    identifier,
                    visibility,
                    ..
                } => (identifier, visibility),
                Expression::Enum {
                    name,
                    variants,
                    visibility,
                    ..
                } => {
                    if exports_all || visibility.is_public() {
                        names.extend(
                            variants
                                .iter()
                                .map(|variant| eco_format!("{}.{}", name, variant.name)),
                        );
                    }
                    (name, visibility)
                }
                _ => continue,
            };
            if exports_all || visibility.is_public() {
                names.push(name.clone());
            }
        }
        names
    }

    /// Redirects `import "{from}"` to `to`, keeping the source spelling as the qualifier.
    pub fn rewrite_import(&mut self, from: &str, to: &str) {
        for item in &mut self.items {
            if let Expression::PackageImport {
                name,
                name_span,
                alias,
                ..
            } = item
                && name.as_str() == from
            {
                if alias.is_none() {
                    *alias = Some(ImportAlias::Named(name.clone(), *name_span));
                }
                *name = to.into();
            }
        }
    }

    pub fn go_filename(&self) -> String {
        go_filename(&self.name)
    }
}

pub fn go_filename(lisette_filename: &str) -> String {
    if let Some(stem) = lisette_filename.strip_suffix(".test.lis") {
        return format!("{stem}_test.go");
    }
    Path::new(lisette_filename)
        .with_extension("go")
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{FileImport, SourceFileKind, Span, go_import_default_name};
    use crate::ast::ImportAlias;

    fn import(raw: &str) -> FileImport {
        let span = Span::new(0, 7, raw.len() as u32 + 2);
        FileImport {
            name: raw.into(),
            name_span: span,
            alias: None,
            span,
        }
    }

    #[test]
    fn an_import_path_is_kept_whole() {
        assert_eq!(
            import("go:github.com/google/uuid").name,
            "go:github.com/google/uuid"
        );
        assert_eq!(
            import("go:github.com/google/uuid@v1.6.0").name,
            "go:github.com/google/uuid@v1.6.0"
        );
        assert_eq!(import("helper@v1.0.0").name, "helper@v1.0.0");
    }

    #[test]
    fn a_blank_alias_is_preserved() {
        let span = Span::new(0, 0, 1);
        let blank = FileImport {
            name: "go:fmt".into(),
            name_span: span,
            alias: Some(ImportAlias::Blank(span)),
            span,
        };

        assert!(matches!(blank.alias, Some(ImportAlias::Blank(_))));
    }

    #[test]
    fn default_names_come_from_the_last_segment() {
        assert_eq!(go_import_default_name("go:github.com/google/uuid"), "uuid");
        assert_eq!(go_import_default_name("go:encoding/json"), "json");
    }

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

    #[test]
    fn only_production_files_emit_go() {
        assert!(SourceFileKind::of("a.lis").emits_go());
        assert!(!SourceFileKind::of("a.d.lis").emits_go());
        assert!(!SourceFileKind::of("a.test.lis").emits_go());
        assert!(!SourceFileKind::of("a.go").emits_go());
    }

    #[test]
    fn source_file_suffixes_are_checked_only_by_source_file_kind() {
        use std::fs;
        use std::path::Path;

        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let patterns = [
            "ends_with(\".lis\")",
            "ends_with(\".d.lis\")",
            "ends_with(\".test.lis\")",
            "ends_with(\"_test.lis\")",
            "== \"lis\"",
        ];
        let mut offenders = Vec::new();
        let mut stack = vec![crates];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    if name != "tests" && name != "target" {
                        stack.push(path);
                    }
                    continue;
                }
                if !name.ends_with(".rs") || name == "build.rs" || path.ends_with("program/file.rs")
                {
                    continue;
                }
                let source = fs::read_to_string(&path).unwrap();
                let production = source.split("#[cfg(test)]").next().unwrap_or("");
                for pattern in patterns {
                    if production.contains(pattern) {
                        offenders.push(format!("{}: {}", path.display(), pattern));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "use SourceFileKind::of: {offenders:#?}"
        );
    }
}
