use rustc_hash::FxHashMap as HashMap;

use crate::go_name;
use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use syntax::ast::{ImportAlias, Span};
use syntax::program::{File, FileImport, ImportKey};

use crate::names::packages::PackageRequirements;

use super::{ImportGroup, OutputImport};

#[derive(Clone, Default)]
pub(crate) struct ImportPlan {
    go_module: String,
    imports: Vec<PlannedImport>,
}

#[derive(Clone)]
struct PlannedImport {
    package: String,
    source_alias: Option<String>,
    qualifier: Option<String>,
    path: String,
    span: Span,
    key: ImportKey,
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
                key: import.key(),
            });
        }

        Self {
            go_module: go_module.to_string(),
            imports,
        }
    }

    pub(crate) fn import_qualifier(&self, package: &str) -> Option<&str> {
        self.imports.iter().rev().find_map(|import| {
            (import.package == package)
                .then_some(import.qualifier.as_deref())
                .flatten()
        })
    }

    pub(crate) fn used_qualifiers(
        &self,
        is_unused: impl Fn(ImportKey) -> bool,
    ) -> impl Iterator<Item = (String, Span)> {
        self.imports.iter().filter_map(move |import| {
            if is_unused(import.key) {
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
        let group = |path: &str| import_group(path, &self.go_module);
        let blank_imports = self
            .imports
            .iter()
            .filter(|import| import.qualifier.is_none())
            .map(|import| OutputImport {
                group: group(&import.path),
                path: import.path.clone(),
                qualifier: None,
            });
        let used_packages = requirements.iter().map(|package| {
            let path = package.package().path();
            OutputImport {
                group: group(path),
                path: path.to_string(),
                qualifier: Some(package.qualifier().to_string()),
            }
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

fn import_group(path: &str, go_module: &str) -> ImportGroup {
    let first_segment = path.split('/').next().unwrap_or(path);
    let in_module = path
        .strip_prefix(go_module)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
    if first_segment.contains('.') || in_module {
        ImportGroup::Other
    } else {
        ImportGroup::Standard
    }
}

pub(crate) fn format_import(path: &str, qualifier: Option<&str>) -> String {
    match qualifier {
        None => format!("_ \"{path}\""),
        Some(qualifier) if qualifier == go_name::go_package_name(path) => format!("\"{path}\""),
        Some(qualifier) => format!("{qualifier} \"{path}\""),
    }
}
