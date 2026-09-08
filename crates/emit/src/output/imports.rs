use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::go_name;
use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use ecow::EcoString;
use syntax::ast::ImportAlias;
use syntax::program::{File, FileImport, PackageId};

use crate::names::packages::{PackageRequirements, PackageUse};
use syntax::program;

use super::OutputImport;

/// Source imports resolved once during the plan phase. Keeping each import as
/// one record avoids synchronizing separate lookup and emission collections.
pub(crate) struct ImportPlan {
    imports: Vec<PlannedImport>,
}

struct PlannedImport {
    package: String,
    source_alias: Option<String>,
    path: String,
    go_alias: String,
    disposition: ImportDisposition,
}

enum ImportDisposition {
    Emit,
    DropUnused,
}

impl ImportPlan {
    pub(crate) fn build(
        file: &File,
        go_module: &str,
        unused_imports: &HashSet<EcoString>,
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
            let disposition = if source_alias
                .as_deref()
                .is_some_and(|alias| unused_imports.contains(alias))
            {
                ImportDisposition::DropUnused
            } else {
                ImportDisposition::Emit
            };
            let (path, go_alias) = resolve_import(&import, go_module, go_package_names);
            let package = import.name.to_string();
            imports.push(PlannedImport {
                package,
                source_alias,
                path,
                go_alias,
                disposition,
            });
        }

        Self { imports }
    }

    pub(crate) fn package_alias(&self, package: &str) -> Option<&str> {
        self.imports.iter().rev().find_map(|import| {
            (import.package == package)
                .then_some(import.source_alias.as_deref())
                .flatten()
        })
    }

    pub(crate) fn package_for_alias(&self, alias: &str) -> Option<&str> {
        self.imports.iter().rev().find_map(|import| {
            (import.source_alias.as_deref() == Some(alias)).then_some(import.package.as_str())
        })
    }
}

pub struct ImportBuilder<'a> {
    go_package_names: &'a HashMap<String, String>,
    go_package_ids: &'a HashSet<String>,
    source_imports: Vec<PlannedImport>,
    requirements: PackageRequirements,
}

impl<'a> ImportBuilder<'a> {
    pub fn new(
        go_package_names: &'a HashMap<String, String>,
        go_package_ids: &'a HashSet<String>,
    ) -> Self {
        Self {
            go_package_names,
            go_package_ids,
            source_imports: Vec::new(),
            requirements: PackageRequirements::default(),
        }
    }

    pub(crate) fn from_plan(
        plan: ImportPlan,
        go_package_names: &'a HashMap<String, String>,
        go_package_ids: &'a HashSet<String>,
    ) -> Self {
        Self {
            source_imports: plan.imports,
            ..Self::new(go_package_names, go_package_ids)
        }
    }

    pub fn extend_with_packages(&mut self, package_ids: &HashSet<PackageId>) {
        for package_id in package_ids {
            let qualifier = self
                .source_imports
                .iter()
                .rev()
                .find(|import| {
                    import.path == package_id.as_str()
                        && matches!(import.disposition, ImportDisposition::DropUnused)
                        && !import.go_alias.is_empty()
                })
                .map(|import| &import.go_alias)
                .or_else(|| {
                    self.go_package_names
                        .get(&format!("{}{package_id}", go_name::GO_IMPORT_PREFIX))
                })
                .cloned()
                .unwrap_or_default();
            self.requirements
                .require(PackageUse::new(package_id.to_string(), qualifier));
        }
    }

    pub(crate) fn extend_with_package_uses(&mut self, requirements: &PackageRequirements) {
        self.requirements.extend(requirements);
    }

    pub fn build(self) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        let mut entries: Vec<OutputImport> = self
            .source_imports
            .iter()
            .filter(|import| {
                matches!(import.disposition, ImportDisposition::Emit)
                    && (import.go_alias == "_"
                        || self
                            .requirements
                            .iter()
                            .any(|used| used.package().path() == import.path))
            })
            .map(|import| OutputImport {
                path: import.path.clone(),
                alias: import.go_alias.clone(),
            })
            .collect();
        for package in self.requirements.iter() {
            let path = package.package().path();
            let qualifier = package.qualifier();
            if entries.iter().any(|entry| {
                entry.path == path
                    && effective_qualifier(path, &entry.alias, self.go_package_ids) == qualifier
            }) {
                continue;
            }
            let alias = self
                .source_imports
                .iter()
                .rev()
                .find(|import| {
                    import.path == path
                        && matches!(import.disposition, ImportDisposition::DropUnused)
                        && !import.go_alias.is_empty()
                        && effective_qualifier(path, &import.go_alias, self.go_package_ids)
                            == qualifier
                })
                .map_or_else(|| qualifier.to_string(), |import| import.go_alias.clone());
            entries.push(OutputImport {
                path: path.to_string(),
                alias,
            });
        }
        entries.sort();
        entries.dedup();
        let diagnostics = detect_collisions(&entries, self.go_package_ids);
        (entries, diagnostics)
    }
}

fn detect_collisions(
    entries: &[OutputImport],
    go_package_ids: &HashSet<String>,
) -> Vec<LisetteDiagnostic> {
    if entries.len() < 2 {
        return Vec::new();
    }
    let mut groups: HashMap<String, Vec<&str>> = HashMap::default();
    for entry in entries {
        if entry.alias == "_" {
            continue;
        }
        let qualifier = effective_qualifier(&entry.path, &entry.alias, go_package_ids);
        groups
            .entry(qualifier)
            .or_default()
            .push(entry.path.as_str());
    }
    let mut groups: Vec<_> = groups.into_iter().filter(|(_, p)| p.len() > 1).collect();
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
        .into_iter()
        .map(|(alias, paths)| {
            let [first, second, rest @ ..] = paths.as_slice() else {
                unreachable!("collision groups contain at least two paths")
            };
            emit_diag::go_import_collision(&alias, first, second, rest)
        })
        .collect()
}

fn effective_qualifier(path: &str, alias: &str, go_package_ids: &HashSet<String>) -> String {
    let package_name = if !alias.is_empty() {
        alias
    } else if go_package_ids.contains(&format!("{}{path}", go_name::GO_IMPORT_PREFIX)) {
        program::go_import_default_name(path)
    } else {
        path.rsplit('/').next().unwrap_or(path)
    };
    go_name::sanitize_package_name(package_name).into_owned()
}

fn resolve_import(
    import: &FileImport,
    go_module: &str,
    go_package_names: &HashMap<String, String>,
) -> (String, String) {
    let go_path = import
        .name
        .strip_prefix(go_name::GO_IMPORT_PREFIX)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/{}", go_module, import.name));

    let go_alias = match &import.alias {
        Some(ImportAlias::Named(a, _)) => a.to_string(),
        Some(ImportAlias::Blank(_)) => "_".to_string(),
        None if go_name::is_go_import(&import.name) => go_package_names
            .get(import.name.as_str())
            .cloned()
            .unwrap_or_default(),
        None => import.effective_alias(go_package_names).unwrap_or_default(),
    };

    (go_path, go_alias)
}

pub(crate) fn format_import(path: &str, alias: &str) -> String {
    let default_name = path.split('/').next_back().unwrap_or(path);

    if alias.is_empty() || alias == default_name {
        let sanitized = go_name::sanitize_package_name(default_name);
        if sanitized != default_name {
            format!("{} \"{path}\"", sanitized)
        } else {
            format!("\"{path}\"")
        }
    } else {
        format!("{alias} \"{path}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::FileParseStatus;

    fn import_plan(source: &str, unused: &[&str]) -> ImportPlan {
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

        ImportPlan::build(
            &file,
            "module",
            &unused.iter().map(|name| (*name).into()).collect(),
            &HashMap::default(),
        )
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
            &[],
        );
        assert_eq!(plan.package_alias("one"), Some("late"));
        assert_eq!(plan.package_for_alias("shared"), Some("three"));
    }

    fn imports_for(source: &str, unused: &[&str], uses: &[(&str, &str)]) -> Vec<OutputImport> {
        let names = HashMap::default();
        let ids = HashSet::default();
        let mut builder = ImportBuilder::from_plan(import_plan(source, unused), &names, &ids);
        let mut requirements = PackageRequirements::default();
        for (path, qualifier) in uses {
            requirements.require(PackageUse::new(*path, *qualifier));
        }
        builder.extend_with_package_uses(&requirements);
        let (imports, diagnostics) = builder.build();
        assert!(diagnostics.is_empty());
        imports
    }

    fn imported(path: &str, alias: &str) -> OutputImport {
        OutputImport {
            path: path.to_string(),
            alias: alias.to_string(),
        }
    }

    #[test]
    fn generated_uses_recover_dropped_aliases_and_deduplicate_qualifiers() {
        let source = r#"import renamed "go:fmt""#;
        let expected = vec![imported("fmt", "fmt"), imported("fmt", "renamed")];
        for uses in [
            vec![("fmt", "renamed"), ("fmt", "fmt"), ("fmt", "renamed")],
            vec![("fmt", "fmt"), ("fmt", "renamed")],
        ] {
            assert_eq!(imports_for(source, &["renamed"], &uses), expected);
        }
    }

    #[test]
    fn blank_import_can_also_be_used_by_generated_code() {
        assert_eq!(
            imports_for(r#"import _ "go:fmt""#, &[], &[("fmt", "fmt")]),
            vec![imported("fmt", "_"), imported("fmt", "fmt")]
        );
    }
}
