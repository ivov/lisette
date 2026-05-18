use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".lisette")
            .join("cache")
            .join(format!(
                "prelude_defs_{:x}_compiler_{:x}.bin",
                PRELUDE_HASH & 0xFFFFFF,
                COMPILER_VERSION_HASH & 0xFFFFFF
            )),
    )
}

pub fn try_load_prelude_cache() -> Option<PreludeCache> {
    let path = cache_path()?;
    let bytes = fs::read(&path).ok()?;
    let cache: PreludeCache = bincode::deserialize(&bytes).ok()?;

    if cache.content_hash != PRELUDE_HASH || cache.compiler_version != COMPILER_VERSION_HASH {
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

pub fn save_prelude_cache(store: &Store) {
    let Some(path) = cache_path() else { return };

    let Some(module) = store.get_module(PRELUDE_MODULE_ID) else {
        return;
    };

    let empty_file_map = HashMap::default();
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

    let cache = PreludeCache {
        content_hash: PRELUDE_HASH,
        compiler_version: COMPILER_VERSION_HASH,
        definitions,
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
            source: stdlib::LIS_PRELUDE_SOURCE.to_string(),
            items: vec![],
        },
    );

    let file_ids: &[u32] = &[];
    let module = store
        .get_module_mut(PRELUDE_MODULE_ID)
        .expect("prelude module must be registered before loading cached definitions");
    for (qualified_name, cached_definition) in cached.definitions {
        let definition = cached_definition.to_definition(file_ids);
        module.definitions.insert(qualified_name.into(), definition);
    }
}
