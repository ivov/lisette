use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::disk;
use super::types::CachedDefinition;
use super::{CACHE_FORMAT_VERSION, COMPILER_VERSION_HASH, PRELUDE_HASH};
use crate::prelude::{PRELUDE_FILE_ID, PRELUDE_MODULE_ID};
use crate::store::Store;

#[derive(Serialize, Deserialize)]
pub struct PreludeCache {
    version: u32,
    content_hash: u64,
    compiler_version: u64,
    definitions: HashMap<String, CachedDefinition>,
}

fn cache_file_name() -> &'static str {
    "prelude_defs.bin"
}

fn cache_path() -> Option<PathBuf> {
    disk::global_path(cache_file_name())
}

pub(crate) fn try_load_prelude_cache() -> Option<PreludeCache> {
    let path = cache_path()?;
    let cache: PreludeCache = disk::read(&path).ok()?;

    if cache.version != CACHE_FORMAT_VERSION
        || cache.content_hash != PRELUDE_HASH
        || cache.compiler_version != COMPILER_VERSION_HASH
    {
        let _ = std::fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

pub(crate) fn save_prelude_cache(store: &Store) {
    let Some(path) = cache_path() else { return };

    let Some(module) = store.get_module(PRELUDE_MODULE_ID) else {
        return;
    };

    let file_id_to_index: HashMap<u32, u32> = [(PRELUDE_FILE_ID, 0)].into_iter().collect();
    let definitions: HashMap<String, CachedDefinition> = module
        .definitions
        .iter()
        .map(|(name, definition)| {
            (
                name.to_string(),
                CachedDefinition::from_definition(definition, &file_id_to_index),
            )
        })
        .collect();

    let cache = PreludeCache {
        version: CACHE_FORMAT_VERSION,
        content_hash: PRELUDE_HASH,
        compiler_version: COMPILER_VERSION_HASH,
        definitions,
    };

    disk::write_global(&path, &cache, "prelude_defs");
}

pub(crate) fn register_cached_prelude(store: &mut Store, cached: PreludeCache) {
    // Register the prelude file for file_id → module_id mapping (needed by diagnostics).
    // Items are empty since we're loading definitions from cache.
    use syntax::program::File;
    store.store_file(File {
        id: PRELUDE_FILE_ID,
        module_id: PRELUDE_MODULE_ID.to_string(),
        name: "prelude.d.lis".to_string(),
        display_path: "prelude.d.lis".to_string(),
        source_path: deps::prelude_typedef_path(),
        source: stdlib::LIS_PRELUDE_SOURCE.to_string(),
        items: vec![],
        file_comment: None,
    });

    let file_ids: &[u32] = &[PRELUDE_FILE_ID];
    let module = store
        .get_module_mut(PRELUDE_MODULE_ID)
        .expect("prelude module must be registered before loading cached definitions");
    for (qualified_name, cached_definition) in cached.definitions {
        cached_definition.install_into(module, qualified_name.into(), file_ids);
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
