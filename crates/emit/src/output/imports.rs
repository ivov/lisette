use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::go_name;
use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use ecow::EcoString;
use syntax::ast::ImportAlias;
use syntax::program::{File, FileImport, ModuleId};

/// Source-derived imports resolved during the plan phase: path -> chosen
/// alias, plus the aliases of imports dropped as unused (still needed so a
/// later generated reference to the same module reuses the source alias).
pub(crate) struct ImportPlan {
    imports: HashMap<String, String>,
    dropped_aliases: HashMap<String, String>,
}

impl ImportPlan {
    pub(crate) fn build(
        file: &File,
        go_module: &str,
        unused_imports: &HashSet<EcoString>,
        go_package_names: &HashMap<String, String>,
    ) -> Self {
        let mut imports = HashMap::default();
        let mut dropped_aliases = HashMap::default();

        for import in file.imports() {
            let is_blank = matches!(import.alias, Some(ImportAlias::Blank(_)));

            if !is_blank
                && let Some(ref alias) = import.effective_alias(go_package_names)
                && unused_imports.contains(alias.as_str())
            {
                let (path, go_alias) = resolve_import(&import, go_module, go_package_names);
                if !go_alias.is_empty() {
                    dropped_aliases.insert(path, go_alias);
                }
                continue;
            }

            let (path, alias) = resolve_import(&import, go_module, go_package_names);
            imports.insert(path, alias);
        }

        Self {
            imports,
            dropped_aliases,
        }
    }
}

pub struct ImportBuilder<'a> {
    go_package_names: &'a HashMap<String, String>,
    go_module_ids: &'a HashSet<String>,
    imports: HashMap<String, String>,
    /// Generated imports of paths the source already imports under another
    /// alias; emitted alongside (Go permits duplicate import paths).
    generated_duplicates: Vec<(String, String)>,
    dropped_aliases: HashMap<String, String>,
    used_modules: HashSet<String>,
}

impl<'a> ImportBuilder<'a> {
    pub fn new(
        go_package_names: &'a HashMap<String, String>,
        go_module_ids: &'a HashSet<String>,
    ) -> Self {
        Self {
            go_package_names,
            go_module_ids,
            imports: HashMap::default(),
            generated_duplicates: Vec::new(),
            dropped_aliases: HashMap::default(),
            used_modules: HashSet::default(),
        }
    }

    pub(crate) fn from_plan(
        plan: &ImportPlan,
        go_package_names: &'a HashMap<String, String>,
        go_module_ids: &'a HashSet<String>,
    ) -> Self {
        Self {
            go_package_names,
            go_module_ids,
            imports: plan.imports.clone(),
            generated_duplicates: Vec::new(),
            dropped_aliases: plan.dropped_aliases.clone(),
            used_modules: HashSet::default(),
        }
    }

    pub fn extend_with_modules(&mut self, module_ids: &HashSet<ModuleId>) {
        for module_id in module_ids {
            let alias = self
                .dropped_aliases
                .get(module_id)
                .or_else(|| {
                    self.go_package_names
                        .get(&format!("{}{module_id}", go_name::GO_IMPORT_PREFIX))
                })
                .cloned()
                .unwrap_or_default();
            self.imports.entry(module_id.clone()).or_insert(alias);
            self.used_modules.insert(module_id.clone());
        }
    }

    /// Record a generated requirement. Generated code references the package
    /// by its fixed canonical qualifier, so a source import of the same path
    /// under another alias is preserved and a second, canonically named
    /// import is added alongside it.
    pub(crate) fn require_generated(&mut self, package: go_name::GeneratedPackage) {
        let path = package.path();
        let canonical = package.qualifier();
        self.used_modules.insert(path.to_string());
        match self.imports.get(path) {
            Some(alias) if effective_package_name(path, alias, self.go_module_ids) == canonical => {
            }
            Some(_) => {
                if !self.generated_duplicates.iter().any(|(p, _)| p == path) {
                    self.generated_duplicates
                        .push((path.to_string(), canonical.to_string()));
                }
            }
            None => {
                self.imports.insert(path.to_string(), canonical.to_string());
            }
        }
    }

    pub fn filter_unused_imports(&mut self) {
        self.imports
            .retain(|path, alias| alias == "_" || self.used_modules.contains(path));
    }

    pub fn build(self) -> (Vec<(String, String)>, Vec<LisetteDiagnostic>) {
        let mut entries: Vec<(String, String)> = self.imports.into_iter().collect();
        entries.extend(self.generated_duplicates);
        entries.sort();
        entries.dedup();
        let diagnostics = detect_collisions(&entries, self.go_module_ids);
        (entries, diagnostics)
    }
}

fn detect_collisions(
    entries: &[(String, String)],
    go_module_ids: &HashSet<String>,
) -> Vec<LisetteDiagnostic> {
    if entries.len() < 2 {
        return Vec::new();
    }
    let mut groups: HashMap<String, Vec<&str>> = HashMap::default();
    for (path, alias) in entries {
        if alias == "_" {
            continue;
        }
        let effective = effective_package_name(path, alias, go_module_ids);
        let sanitized = go_name::sanitize_package_name(effective).into_owned();
        groups.entry(sanitized).or_default().push(path.as_str());
    }
    let mut groups: Vec<_> = groups.into_iter().filter(|(_, p)| p.len() > 1).collect();
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
        .into_iter()
        .map(|(alias, paths)| {
            let owned: Vec<String> = paths.into_iter().map(str::to_string).collect();
            emit_diag::go_import_collision(&alias, &owned)
        })
        .collect()
}

fn effective_package_name<'a>(
    path: &'a str,
    alias: &'a str,
    go_module_ids: &HashSet<String>,
) -> &'a str {
    if !alias.is_empty() {
        return alias;
    }
    if go_module_ids.contains(&format!("{}{path}", go_name::GO_IMPORT_PREFIX)) {
        syntax::program::go_import_default_name(path)
    } else {
        path.rsplit('/').next().unwrap_or(path)
    }
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
