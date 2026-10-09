use rustc_hash::FxHashMap as HashMap;

use crate::go_name;
use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use syntax::ast::{ImportAlias, Span};
use syntax::program::{File, FileImport};

use crate::names::packages::PackageRequirements;

use super::OutputImport;

/// Source imports resolved once during the plan phase. Keeping each import as
/// one record avoids synchronizing separate lookup and emission collections.
#[derive(Default)]
pub(crate) struct ImportPlan {
    imports: Vec<PlannedImport>,
}

struct PlannedImport {
    package: String,
    source_alias: Option<String>,
    qualifier: Option<String>,
    path: String,
    span: Span,
    /// Keys the import in the unused facts.
    name_span: Span,
}

impl ImportPlan {
    pub(crate) fn build(
        file: &File,
        go_module: &str,
        go_package_names: &HashMap<String, String>,
    ) -> Self {
        let mut imports = Vec::new();

        for import in file.imports() {
            let is_blank = matches!(import.alias, Some(ImportAlias::Blank(_)));
            let source_alias = if is_blank {
                None
            } else {
                import.effective_alias(go_package_names)
            };
            let qualifier = source_import_qualifier(&import, go_package_names);
            let path = import_go_path(&import, go_module);
            let package = import.name.to_string();
            let span = match &import.alias {
                Some(ImportAlias::Named(_, span)) => *span,
                _ => import.name_span,
            };
            imports.push(PlannedImport {
                package,
                source_alias,
                qualifier,
                path,
                span,
                name_span: import.name_span,
            });
        }

        Self { imports }
    }

    pub(crate) fn import_qualifier(&self, package: &str) -> Option<&str> {
        self.imports.iter().rev().find_map(|import| {
            (import.package == package)
                .then_some(import.qualifier.as_deref())
                .flatten()
        })
    }

    /// Non-blank used imports as qualifier and span, in source order.
    pub(crate) fn used_qualifiers(
        &self,
        is_unused: impl Fn(&Span) -> bool,
    ) -> impl Iterator<Item = (String, Span)> {
        self.imports.iter().filter_map(move |import| {
            if is_unused(&import.name_span) {
                return None;
            }
            Some((import.qualifier.clone()?, import.span))
        })
    }

    pub(crate) fn package_for_alias(&self, alias: &str) -> Option<&str> {
        self.imports.iter().rev().find_map(|import| {
            (import.source_alias.as_deref() == Some(alias)).then_some(import.package.as_str())
        })
    }

    pub(crate) fn finish(
        self,
        requirements: &PackageRequirements,
    ) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        let blank_imports = self
            .imports
            .iter()
            .filter(|import| import.qualifier.is_none())
            .map(|import| OutputImport {
                path: import.path.clone(),
                qualifier: None,
            });
        let used_packages = requirements.iter().map(|package| OutputImport {
            path: package.package().path().to_string(),
            qualifier: Some(package.qualifier().to_string()),
        });
        let mut entries: Vec<OutputImport> = blank_imports.chain(used_packages).collect();
        entries.sort();
        entries.dedup();
        let diagnostics = detect_collisions(&entries);
        (entries, diagnostics)
    }
}

fn detect_collisions(entries: &[OutputImport]) -> Vec<LisetteDiagnostic> {
    if entries.len() < 2 {
        return Vec::new();
    }
    let mut groups: HashMap<&str, Vec<&str>> = HashMap::default();
    for entry in entries {
        if let Some(qualifier) = &entry.qualifier {
            groups
                .entry(qualifier.as_str())
                .or_default()
                .push(entry.path.as_str());
        }
    }
    let mut groups: Vec<_> = groups.into_iter().filter(|(_, p)| p.len() > 1).collect();
    groups.sort_by(|a, b| a.0.cmp(b.0));
    groups
        .into_iter()
        .map(|(alias, paths)| {
            let [first, second, rest @ ..] = paths.as_slice() else {
                unreachable!("collision groups contain at least two paths")
            };
            emit_diag::go_import_collision(alias, first, second, rest)
        })
        .collect()
}

fn source_import_qualifier(
    import: &FileImport,
    go_package_names: &HashMap<String, String>,
) -> Option<String> {
    if matches!(import.alias, Some(ImportAlias::Blank(_))) {
        return None;
    }
    let alias = import.effective_alias(go_package_names)?;
    let path = import
        .name
        .strip_prefix(go_name::GO_IMPORT_PREFIX)
        .unwrap_or(&import.name);
    Some(rendered_qualifier(path, alias))
}

pub(crate) fn rendered_qualifier(path: &str, name: String) -> String {
    if name == go_name::go_package_name(path) {
        go_name::sanitize_package_name(&name).into_owned()
    } else {
        name
    }
}

fn import_go_path(import: &FileImport, go_module: &str) -> String {
    import
        .name
        .strip_prefix(go_name::GO_IMPORT_PREFIX)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/{}", go_module, import.name))
}

/// Renders one import spec, or a blank import for `None`.
pub(crate) fn format_import(path: &str, qualifier: Option<&str>) -> String {
    match qualifier {
        None => format!("_ \"{path}\""),
        Some(qualifier) if qualifier == go_name::go_package_name(path) => format!("\"{path}\""),
        Some(qualifier) => format!("{qualifier} \"{path}\""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::packages::PackageUse;
    use syntax::FileParseStatus;

    fn import_plan(source: &str) -> ImportPlan {
        let parsed = syntax::build_ast(source, 0);
        assert!(parsed.errors.is_empty());
        let file = File {
            id: 0,
            package_id: "package".to_string(),
            parse_status: FileParseStatus::Clean,
            name: "test.lis".to_string(),
            display_path: "test.lis".to_string(),
            source_path: None,
            source: source.to_string(),
            items: parsed.ast,
            file_comment: None,
        };

        ImportPlan::build(&file, "module", &HashMap::default())
    }

    #[test]
    fn import_plan_resolves_the_last_matching_alias() {
        let plan = import_plan(
            r#"
import early "one"
import late "one"
import shared "two"
import shared "three"
"#,
        );
        assert_eq!(plan.import_qualifier("one"), Some("late"));
        assert_eq!(plan.package_for_alias("shared"), Some("three"));
    }

    #[test]
    fn used_qualifiers_skip_blank_and_unused_imports() {
        let source = r#"
import _ "go:fmt"
import unused "one"
import kept "two"
import "three"
"#;
        let plan = import_plan(source);
        let unused_path = source.find("\"one\"").unwrap() as u32;
        let qualifiers: Vec<_> = plan
            .used_qualifiers(|span| span.byte_offset == unused_path)
            .map(|(qualifier, span)| {
                let start = span.byte_offset as usize;
                let end = start + span.byte_length as usize;
                (qualifier, &source[start..end])
            })
            .collect();
        assert_eq!(
            qualifiers,
            vec![
                ("kept".to_string(), "kept"),
                ("three".to_string(), "\"three\""),
            ]
        );
    }

    fn finish_with(
        source: &str,
        uses: &[(&str, &str)],
    ) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        let mut requirements = PackageRequirements::default();
        for (path, qualifier) in uses {
            requirements.require(PackageUse::new(*path, *qualifier));
        }
        import_plan(source).finish(&requirements)
    }

    fn imports_for(source: &str, uses: &[(&str, &str)]) -> Vec<OutputImport> {
        let (imports, diagnostics) = finish_with(source, uses);
        assert!(diagnostics.is_empty());
        imports
    }

    fn collisions(source: &str, uses: &[(&str, &str)]) -> Vec<LisetteDiagnostic> {
        let (_, diagnostics) = finish_with(source, uses);
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.code_str(), Some("emit.go_import_collision"));
        }
        diagnostics
    }

    fn imported(path: &str, qualifier: Option<&str>) -> OutputImport {
        OutputImport {
            path: path.to_string(),
            qualifier: qualifier.map(str::to_string),
        }
    }

    #[test]
    fn generated_uses_keep_source_aliases_and_deduplicate_qualifiers() {
        let source = r#"import renamed "go:fmt""#;
        let expected = vec![
            imported("fmt", Some("fmt")),
            imported("fmt", Some("renamed")),
        ];
        for uses in [
            vec![("fmt", "renamed"), ("fmt", "fmt"), ("fmt", "renamed")],
            vec![("fmt", "fmt"), ("fmt", "renamed")],
        ] {
            assert_eq!(imports_for(source, &uses), expected);
        }
    }

    #[test]
    fn blank_import_can_also_be_used_by_generated_code() {
        assert_eq!(
            imports_for(r#"import _ "go:fmt""#, &[("fmt", "fmt")]),
            vec![imported("fmt", None), imported("fmt", Some("fmt"))]
        );
    }

    #[test]
    fn go_import_collision_flags_shared_qualifier() {
        let diagnostics = collisions(
            "",
            &[("database/sql", "sql"), ("entgo.io/ent/dialect/sql", "sql")],
        );
        assert_eq!(diagnostics.len(), 1);
        let help = diagnostics[0].plain_help().unwrap_or_default();
        assert!(help.contains("database/sql") && help.contains("entgo.io/ent/dialect/sql"));
    }

    #[test]
    fn go_import_collision_silent_when_qualifiers_differ() {
        let diagnostics = collisions(
            "",
            &[
                ("database/sql", "sql"),
                ("entgo.io/ent/dialect/sql", "entsql"),
            ],
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn go_import_collision_silent_for_distinct_versioned_packages() {
        let diagnostics = collisions(
            "",
            &[
                ("github.com/pion/sdp/v3", "sdp"),
                ("github.com/pion/dtls/v3", "dtls"),
            ],
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn go_import_collision_flags_local_packages_sharing_last_segment() {
        let diagnostics = collisions(
            "",
            &[("myproject/api/v2", "v2"), ("myproject/admin/v2", "v2")],
        );
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn unaliased_go_import_under_project_package_resolves_by_package_name() {
        let (imports, diagnostics) = finish_with(
            r#"import "go:myproject/plugins/v2""#,
            &[
                ("myproject/plugins/v2", "plugins"),
                ("myproject/api/v2", "v2"),
            ],
        );
        assert!(diagnostics.is_empty());
        assert_eq!(
            imports,
            vec![
                imported("myproject/api/v2", Some("v2")),
                imported("myproject/plugins/v2", Some("plugins")),
            ]
        );
    }
}
