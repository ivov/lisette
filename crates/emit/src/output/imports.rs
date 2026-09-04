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
    package_aliases: HashMap<String, usize>,
    alias_packages: HashMap<String, usize>,
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
        let mut package_aliases = HashMap::default();
        let mut alias_packages = HashMap::default();

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
            let index = imports.len();
            if let Some(alias) = &source_alias {
                package_aliases.insert(package.clone(), index);
                alias_packages.insert(alias.clone(), index);
            }
            imports.push(PlannedImport {
                package,
                source_alias,
                path,
                go_alias,
                disposition,
            });
        }

        Self {
            imports,
            package_aliases,
            alias_packages,
        }
    }

    pub(crate) fn package_alias(&self, package: &str) -> Option<&str> {
        self.package_aliases
            .get(package)
            .and_then(|index| self.imports[*index].source_alias.as_deref())
    }

    pub(crate) fn package_for_alias(&self, alias: &str) -> Option<&str> {
        self.alias_packages
            .get(alias)
            .map(|index| self.imports[*index].package.as_str())
    }

    fn into_builder_state(self) -> (HashMap<String, String>, HashMap<String, String>) {
        let mut imports = HashMap::default();
        let mut dropped_aliases = HashMap::default();
        for import in self.imports {
            match import.disposition {
                ImportDisposition::Emit => {
                    imports.insert(import.path, import.go_alias);
                }
                ImportDisposition::DropUnused if !import.go_alias.is_empty() => {
                    dropped_aliases.insert(import.path, import.go_alias);
                }
                ImportDisposition::DropUnused => {}
            }
        }
        (imports, dropped_aliases)
    }
}

pub struct ImportBuilder<'a> {
    go_package_names: &'a HashMap<String, String>,
    go_package_ids: &'a HashSet<String>,
    imports: HashMap<String, String>,
    /// Additional qualifiers requested for a path already present under a
    /// different qualifier.
    duplicate_imports: HashSet<(String, String)>,
    dropped_aliases: HashMap<String, String>,
    used_packages: HashSet<String>,
}

impl<'a> ImportBuilder<'a> {
    pub fn new(
        go_package_names: &'a HashMap<String, String>,
        go_package_ids: &'a HashSet<String>,
    ) -> Self {
        Self {
            go_package_names,
            go_package_ids,
            imports: HashMap::default(),
            duplicate_imports: HashSet::default(),
            dropped_aliases: HashMap::default(),
            used_packages: HashSet::default(),
        }
    }

    pub(crate) fn from_plan(
        plan: ImportPlan,
        go_package_names: &'a HashMap<String, String>,
        go_package_ids: &'a HashSet<String>,
    ) -> Self {
        let (imports, dropped_aliases) = plan.into_builder_state();
        Self {
            go_package_names,
            go_package_ids,
            imports,
            duplicate_imports: HashSet::default(),
            dropped_aliases,
            used_packages: HashSet::default(),
        }
    }

    pub fn extend_with_packages(&mut self, package_ids: &HashSet<PackageId>) {
        for package_id in package_ids {
            let qualifier = self
                .dropped_aliases
                .get(package_id)
                .or_else(|| {
                    self.go_package_names
                        .get(&format!("{}{package_id}", go_name::GO_IMPORT_PREFIX))
                })
                .cloned()
                .unwrap_or_default();
            self.require_package_use(&PackageUse::new(package_id.clone(), qualifier));
        }
    }

    pub(crate) fn extend_with_package_uses(&mut self, requirements: &PackageRequirements) {
        for package in requirements.iter() {
            self.require_package_use(package);
        }
    }

    fn require_package_use(&mut self, package: &PackageUse) {
        let path = package.package().path();
        let qualifier = package.qualifier();
        self.used_packages.insert(path.to_string());
        match self.imports.get(path) {
            Some(alias) if effective_qualifier(path, alias, self.go_package_ids) == qualifier => {}
            Some(_) => {
                self.duplicate_imports
                    .insert((path.to_string(), qualifier.to_string()));
            }
            None => {
                let alias = self
                    .dropped_aliases
                    .get(path)
                    .filter(|alias| {
                        effective_qualifier(path, alias, self.go_package_ids) == qualifier
                    })
                    .cloned()
                    .unwrap_or_else(|| qualifier.to_string());
                self.imports.insert(path.to_string(), alias);
            }
        }
    }

    pub fn build(mut self) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        self.imports
            .retain(|path, alias| alias == "_" || self.used_packages.contains(path));
        let mut entries: Vec<OutputImport> = self
            .imports
            .into_iter()
            .map(|(path, alias)| OutputImport { path, alias })
            .collect();
        entries.extend(
            self.duplicate_imports
                .into_iter()
                .map(|(path, alias)| OutputImport { path, alias }),
        );
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

    #[test]
    fn import_plan_indexes_the_last_matching_alias() {
        let source = r#"
import early "one"
import late "one"
import shared "two"
import shared "three"
"#;
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

        let plan = ImportPlan::build(&file, "module", &HashSet::default(), &HashMap::default());

        assert_eq!(plan.package_alias("one"), Some("late"));
        assert_eq!(plan.package_for_alias("shared"), Some("three"));
    }
}
