use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::go_name;
use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use ecow::EcoString;
use syntax::ast::ImportAlias;
use syntax::program::{File, FileImport, ModuleId};

pub struct ImportBuilder<'a> {
    go_module: &'a str,
    unused_imports: &'a HashSet<EcoString>,
    go_package_names: &'a HashMap<String, String>,
    imports: HashMap<String, String>,
    dropped_aliases: HashMap<String, String>,
}

impl<'a> ImportBuilder<'a> {
    pub fn new(
        go_module: &'a str,
        unused_imports: &'a HashSet<EcoString>,
        go_package_names: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            go_module,
            unused_imports,
            go_package_names,
            imports: HashMap::default(),
            dropped_aliases: HashMap::default(),
        }
    }

    pub fn collect_from_file(&mut self, file: &File) {
        for import in file.imports() {
            let is_blank = matches!(import.alias, Some(ImportAlias::Blank(_)));

            if !is_blank
                && let Some(ref alias) = import.effective_alias(self.go_package_names)
                && self.unused_imports.contains(alias.as_str())
            {
                let (path, go_alias) =
                    resolve_import(&import, self.go_module, self.go_package_names);
                if !go_alias.is_empty() {
                    self.dropped_aliases.insert(path, go_alias);
                }
                continue;
            }

            let (path, alias) = resolve_import(&import, self.go_module, self.go_package_names);
            self.imports.insert(path, alias);
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
        }
    }

    pub fn require_fmt(&mut self) {
        self.imports.insert("fmt".to_string(), "fmt".to_string());
    }

    pub fn require_stdlib(&mut self) {
        self.imports.insert(
            go_name::PRELUDE_IMPORT_PATH.to_string(),
            "lisette".to_string(),
        );
    }

    pub fn require_errors(&mut self) {
        self.imports
            .insert("errors".to_string(), "errors".to_string());
    }

    pub fn require_slices(&mut self) {
        self.imports
            .insert("slices".to_string(), "slices".to_string());
    }

    pub fn require_strings(&mut self) {
        self.imports
            .insert("strings".to_string(), "strings".to_string());
    }

    pub fn require_maps(&mut self) {
        self.imports.insert("maps".to_string(), "maps".to_string());
    }

    /// This handles cases where a cross-module type alias resolves to a native
    /// Go type, erasing the reference to the imported module.
    pub fn filter_unreferenced(&mut self, source: &str) {
        self.imports.retain(|path, alias| {
            if alias == "_" {
                return true;
            }
            let escaped = go_name::escape_reserved(effective_package_name(path, alias));
            let pattern = format!("{escaped}.");
            source.contains(&pattern)
        });
    }

    pub fn build(self) -> (HashMap<String, String>, Vec<LisetteDiagnostic>) {
        let diagnostics = self.detect_collisions();
        (self.imports, diagnostics)
    }

    fn detect_collisions(&self) -> Vec<LisetteDiagnostic> {
        if self.imports.len() < 2 {
            return Vec::new();
        }
        let mut groups: HashMap<String, Vec<&str>> = HashMap::default();
        for (path, alias) in &self.imports {
            if alias == "_" {
                continue;
            }
            let effective = effective_package_name(path, alias);
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
}

fn effective_package_name<'a>(path: &'a str, alias: &'a str) -> &'a str {
    if alias.is_empty() {
        path.rsplit('/').next().unwrap_or(path)
    } else {
        alias
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
