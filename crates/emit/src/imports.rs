use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::go::go_name;
use ecow::EcoString;
use syntax::ast::ImportAlias;
use syntax::program::{File, FileImport, ModuleId};

pub struct ImportBuilder<'a> {
    go_module: &'a str,
    unused_imports: &'a HashSet<EcoString>,
    imports: HashMap<String, String>,
    dropped_aliases: HashMap<String, String>,
}

impl<'a> ImportBuilder<'a> {
    pub fn new(go_module: &'a str, unused_imports: &'a HashSet<EcoString>) -> Self {
        Self {
            go_module,
            unused_imports,
            imports: HashMap::default(),
            dropped_aliases: HashMap::default(),
        }
    }

    pub fn collect_from_file(&mut self, file: &File) {
        for import in file.imports() {
            let is_blank = matches!(import.alias, Some(ImportAlias::Blank(_)));

            if !is_blank
                && let Some(ref alias) = import.effective_alias()
                && self.unused_imports.contains(alias.as_str())
            {
                let (path, go_alias) = resolve_import(&import, self.go_module);
                if !go_alias.is_empty() {
                    self.dropped_aliases.insert(path, go_alias);
                }
                continue;
            }

            let (path, alias) = resolve_import(&import, self.go_module);
            self.imports.insert(path, alias);
        }
    }

    pub fn extend_with_modules(&mut self, module_ids: &HashSet<ModuleId>) {
        for module_id in module_ids {
            if let Some(alias) = self.dropped_aliases.get(module_id) {
                self.imports
                    .entry(module_id.clone())
                    .or_insert_with(|| alias.clone());
            } else {
                self.imports.entry(module_id.clone()).or_default();
            }
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

            let pkg_name = if alias.is_empty() {
                // No alias — use last path component
                path.rsplit('/').next().unwrap_or(path)
            } else {
                alias.as_str()
            };

            let escaped = go_name::escape_reserved(pkg_name);
            let pattern = format!("{escaped}.");
            source.contains(&pattern)
        });
    }

    pub fn build(self) -> HashMap<String, String> {
        self.imports
    }
}

fn resolve_import(import: &FileImport, go_module: &str) -> (String, String) {
    let go_path = import
        .name
        .strip_prefix(go_name::GO_IMPORT_PREFIX)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/{}", go_module, import.name));

    let go_alias = match &import.alias {
        Some(ImportAlias::Named(a, _)) => a.to_string(),
        Some(ImportAlias::Blank(_)) => "_".to_string(),
        None if go_name::is_go_import(&import.name) => String::new(),
        None => import.effective_alias().unwrap_or_default(),
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
