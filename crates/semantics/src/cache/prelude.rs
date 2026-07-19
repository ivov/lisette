use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::disk;
use super::types::CachedDefinition;
use super::{COMPILER_VERSION_HASH, PRELUDE_HASH};
use crate::prelude::{PRELUDE_FILE_ID, PRELUDE_MODULE_ID};
use crate::store::Store;

#[derive(Serialize, Deserialize)]
pub struct PreludeCache {
    pub content_hash: u64,
    pub compiler_version: u64,
    pub definitions: HashMap<String, CachedDefinition>,
}

fn cache_file_name() -> &'static str {
    "prelude_defs.bin"
}

fn cache_path() -> Option<PathBuf> {
    disk::global_path(cache_file_name())
}

pub fn try_load_prelude_cache() -> Option<PreludeCache> {
    let path = cache_path()?;
    let cache: PreludeCache = disk::read(&path).ok()?;

    if cache.content_hash != PRELUDE_HASH || cache.compiler_version != COMPILER_VERSION_HASH {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

pub fn save_prelude_cache(store: &Store) {
    let Some(path) = cache_path() else { return };

    let Some(module) = store.get_module(PRELUDE_MODULE_ID) else {
        return;
    };

    let file_id_to_index: HashMap<u32, u32> = [(PRELUDE_FILE_ID, 0)].into_iter().collect();
    let definitions: HashMap<String, CachedDefinition> = module
        .definitions
        .iter()
        .map(|(name, definition)| {
            let is_const = module.const_names.contains(name);
            (
                name.to_string(),
                CachedDefinition::from_definition(definition, is_const, &file_id_to_index),
            )
        })
        .collect();

    let cache = PreludeCache {
        content_hash: PRELUDE_HASH,
        compiler_version: COMPILER_VERSION_HASH,
        definitions,
    };

    disk::write_global(&path, &cache, "prelude_defs");
}

pub fn register_cached_prelude(store: &mut Store, cached: PreludeCache) {
    store.mark_visited(PRELUDE_MODULE_ID);

    // Register the prelude file for file_id → module_id mapping (needed by diagnostics).
    // Items are empty since we're loading definitions from cache.
    use syntax::program::File;
    store.store_file(
        PRELUDE_MODULE_ID,
        File {
            id: PRELUDE_FILE_ID,
            module_id: PRELUDE_MODULE_ID.to_string(),
            name: "prelude.d.lis".to_string(),
            display_path: "prelude.d.lis".to_string(),
            source: stdlib::LIS_PRELUDE_SOURCE.to_string(),
            items: vec![],
            file_comment: None,
        },
    );

    let file_ids: &[u32] = &[PRELUDE_FILE_ID];
    let module = store
        .get_module_mut(PRELUDE_MODULE_ID)
        .expect("prelude module must be registered before loading cached definitions");
    for (qualified_name, cached_definition) in cached.definitions {
        cached_definition.install_into(module, qualified_name.into(), file_ids);
    }

    if let Some(path) = deps::prelude_typedef_path() {
        store.typedef_paths.insert(PRELUDE_FILE_ID, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_file_name_is_stable() {
        assert_eq!(cache_file_name(), "prelude_defs.bin");
    }
}
