use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use stdlib::{Target, get_go_stdlib_typedef};

use super::disk;
use super::types::CachedDefinition;
use super::{CACHE_FORMAT_VERSION, COMPILER_VERSION_HASH, GO_STDLIB_HASH};
use crate::checker::registration::extract_package_directive;
use crate::store::Store;
use syntax::program::File;

#[derive(Serialize, Deserialize)]
pub struct GoStdlibCache {
    version: u32,
    content_hash: u64,
    compiler_version: u64,
    pub modules: HashMap<String, GoModuleCache>,
}

#[derive(Serialize, Deserialize)]
pub struct GoModuleCache {
    pub definitions: HashMap<String, CachedDefinition>,
    /// Go module imports (e.g., `["go:io", "go:sync"]`).
    go_imports: Vec<String>,
}

fn cache_file_name(target: Target) -> String {
    format!("stdlib_defs_{}.bin", target.cache_segment())
}

fn cache_path(target: Target) -> Option<PathBuf> {
    disk::global_path(&cache_file_name(target))
}

pub fn try_load_go_stdlib_cache(target: Target) -> Option<GoStdlibCache> {
    let path = cache_path(target)?;
    let cache: GoStdlibCache = disk::read(&path).ok()?;

    if cache.version != CACHE_FORMAT_VERSION
        || cache.content_hash != GO_STDLIB_HASH
        || cache.compiler_version != COMPILER_VERSION_HASH
    {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

pub(crate) fn save_go_stdlib_cache(store: &Store, go_module_ids: &[String], target: Target) {
    let Some(path) = cache_path(target) else {
        return;
    };

    let mut modules = HashMap::default();
    // Go definitions don't reference files, so file_id_to_index is always empty.
    let empty_file_map = HashMap::default();
    for module_id in go_module_ids {
        let Some(module) = store.get_module(module_id) else {
            continue;
        };
        let definitions: HashMap<String, CachedDefinition> = module
            .definitions
            .iter()
            .map(|(name, definition)| {
                (
                    name.to_string(),
                    CachedDefinition::from_definition(definition, &empty_file_map),
                )
            })
            .collect();

        let go_imports = get_go_imports_from_source(module_id, target);

        modules.insert(
            module_id.clone(),
            GoModuleCache {
                definitions,
                go_imports,
            },
        );
    }

    let cache = GoStdlibCache {
        version: CACHE_FORMAT_VERSION,
        content_hash: GO_STDLIB_HASH,
        compiler_version: COMPILER_VERSION_HASH,
        modules,
    };

    disk::write_global(&path, &cache, "stdlib_defs");
}

/// Load a Go module and its transitive deps from cache, recursively.
pub(crate) fn load_cached_go_module(
    store: &mut Store,
    module_id: &str,
    cache: &GoStdlibCache,
    target: Target,
) {
    if store.has(module_id) {
        return;
    }

    let Some(cached) = cache.modules.get(module_id) else {
        return;
    };

    // Presence is the recursion guard as well as the canonical loaded-module fact.
    store.add_module(module_id);

    // Load transitive deps first
    let imports = cached.go_imports.clone();
    for dep in &imports {
        load_cached_go_module(store, dep, cache, target);
    }

    register_cached_go_module(store, module_id, cached, target);
}

fn register_cached_go_module(
    store: &mut Store,
    module_id: &str,
    cached: &GoModuleCache,
    target: Target,
) {
    let go_pkg = module_id.strip_prefix("go:");
    let source = go_pkg.and_then(|go_pkg| get_go_stdlib_typedef(go_pkg, target));

    if let Some(source) = source
        && let Some(pkg_name) = extract_package_directive(source)
    {
        store
            .go_package_names
            .insert(module_id.to_string(), pkg_name);
    }

    // Register the typedef File and its on-disk path so go-to-definition can
    // navigate. The files are written by the LSP at startup.
    let owned_file_id;
    let mut file_ids: &[u32] = &[];
    if let (Some(go_pkg), Some(source)) = (go_pkg, source) {
        let file_id = store.new_file_id();
        let filename = format!("{}.d.lis", go_pkg.replace('/', "_"));
        let mut file = File::new_cached(module_id, &filename, &filename, source, file_id);
        file.source_path = deps::stdlib_typedef_path(target, go_pkg);
        store.store_file(file);
        owned_file_id = [file_id];
        file_ids = &owned_file_id;
    }

    let module = store.get_module_mut(module_id).unwrap();
    for (qualified_name, cached_definition) in &cached.definitions {
        cached_definition.install_into(module, qualified_name.clone().into(), file_ids);
    }
}

/// Extract Go imports from a module's `.d.lis` source without parsing.
fn get_go_imports_from_source(module_id: &str, target: Target) -> Vec<String> {
    let Some(go_pkg) = module_id.strip_prefix("go:") else {
        return vec![];
    };
    let Some(source) = get_go_stdlib_typedef(go_pkg, target) else {
        return vec![];
    };
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("import \"go:")?;
            let pkg = rest.strip_suffix('"')?;
            Some(format!("go:{pkg}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_file_name_includes_target_only() {
        let target = Target::new("darwin", "arm64");
        assert_eq!(cache_file_name(target), "stdlib_defs_darwin_arm64.bin");
    }

    #[test]
    fn module_presence_terminates_cached_import_cycles() {
        let module = |import: &str| GoModuleCache {
            definitions: HashMap::default(),
            go_imports: vec![import.to_string()],
        };
        let cache = GoStdlibCache {
            version: CACHE_FORMAT_VERSION,
            content_hash: GO_STDLIB_HASH,
            compiler_version: COMPILER_VERSION_HASH,
            modules: [
                ("go:example/a".to_string(), module("go:example/b")),
                ("go:example/b".to_string(), module("go:example/a")),
            ]
            .into_iter()
            .collect(),
        };
        let mut store = Store::new();

        load_cached_go_module(&mut store, "go:example/a", &cache, Target::host());

        assert!(store.has("go:example/a"));
        assert!(store.has("go:example/b"));
    }
}
