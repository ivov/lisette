use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use stdlib::{Target, get_go_stdlib_typedef};

use super::types::CachedDefinition;
use super::{COMPILER_VERSION_HASH, GO_STDLIB_HASH};
use crate::checker::registration::extract_package_directive;
use crate::store::Store;

#[derive(Serialize, Deserialize)]
pub struct GoStdlibCache {
    pub content_hash: u64,
    pub compiler_version: u64,
    pub modules: HashMap<String, GoModuleCache>,
}

#[derive(Serialize, Deserialize)]
pub struct GoModuleCache {
    pub definitions: HashMap<String, CachedDefinition>,
    /// Go module imports (e.g., `["go:io", "go:sync"]`).
    pub go_imports: Vec<String>,
}

fn cache_path(target: Target) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".lisette")
            .join("cache")
            .join(format!(
                "stdlib_defs_{:x}_compiler_{:x}_{}_{}.bin",
                GO_STDLIB_HASH & 0xFFFFFF,
                COMPILER_VERSION_HASH & 0xFFFFFF,
                target.goos,
                target.goarch,
            )),
    )
}

pub fn try_load_go_stdlib_cache(target: Target) -> Option<GoStdlibCache> {
    let path = cache_path(target)?;
    let bytes = fs::read(&path).ok()?;
    let cache: GoStdlibCache = bincode::deserialize(&bytes).ok()?;

    if cache.content_hash != GO_STDLIB_HASH || cache.compiler_version != COMPILER_VERSION_HASH {
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

pub fn save_go_stdlib_cache(store: &Store, go_module_ids: &[String], target: Target) {
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
        content_hash: GO_STDLIB_HASH,
        compiler_version: COMPILER_VERSION_HASH,
        modules,
    };

    let Ok(bytes) = bincode::serialize(&cache) else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let temp_path = path.with_extension("bin.tmp");
    if fs::write(&temp_path, bytes).is_ok() {
        let _ = fs::rename(&temp_path, &path);
    }
}

/// Load a Go module and its transitive deps from cache, recursively.
pub fn load_cached_go_module(
    store: &mut Store,
    module_id: &str,
    cache: &GoStdlibCache,
    target: Target,
) {
    if store.is_visited(module_id) {
        return;
    }

    let Some(cached) = cache.modules.get(module_id) else {
        return;
    };

    // Load transitive deps first
    let imports = cached.go_imports.clone();
    for dep in &imports {
        load_cached_go_module(store, dep, cache, target);
    }

    if store.is_visited(module_id) {
        return; // May have been loaded as a transitive dep of a sibling
    }

    register_cached_go_module(store, module_id, cached, target);
}

fn register_cached_go_module(
    store: &mut Store,
    module_id: &str,
    cached: &GoModuleCache,
    target: Target,
) {
    store.add_module(module_id);
    store.mark_visited(module_id);

    if let Some(go_pkg) = module_id.strip_prefix("go:")
        && let Some(source) = get_go_stdlib_typedef(go_pkg, target)
        && let Some(pkg_name) = extract_package_directive(source)
        && module_id.rsplit('/').next() != Some(pkg_name.as_str())
    {
        store
            .go_package_names
            .insert(module_id.to_string(), pkg_name);
    }

    // Go modules don't need files registered — they're internal and filtered out
    // of diagnostic rendering. We use an empty file_ids slice for span restoration
    // (all spans will get file_id 0, which is fine for Go stdlib).
    let file_ids: &[u32] = &[];

    let module = store.get_module_mut(module_id).unwrap();
    for (qualified_name, cached_definition) in &cached.definitions {
        let definition = cached_definition.to_definition(file_ids);
        module
            .definitions
            .insert(qualified_name.clone().into(), definition);
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
