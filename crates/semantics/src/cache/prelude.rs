use rustc_hash::FxHashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::disk;
use super::types::CachedDefinition;
use super::{CACHE_FORMAT_VERSION, COMPILER_BUILD_HASH, PRELUDE_HASH};
use crate::prelude::PRELUDE_FILE_ID;
use crate::store::Store;
use std::fs;
use syntax::program::PRELUDE_PACKAGE_ID;

#[derive(Serialize, Deserialize)]
pub struct PreludeCache {
    version: u32,
    content_hash: u64,
    compiler_version: u64,
    definitions: FxHashMap<String, CachedDefinition>,
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
        || cache.compiler_version != COMPILER_BUILD_HASH
    {
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(cache)
}

pub(crate) fn save_prelude_cache(store: &Store) {
    let Some(path) = cache_path() else { return };

    let Some(package) = store.get_package(PRELUDE_PACKAGE_ID) else {
        return;
    };

    let file_id_to_index: FxHashMap<u32, u32> = [(PRELUDE_FILE_ID, 0)].into_iter().collect();
    let definitions: FxHashMap<String, CachedDefinition> = package
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
        compiler_version: COMPILER_BUILD_HASH,
        definitions,
    };

    disk::write_global(&path, &cache, "prelude_defs");
}

pub(crate) fn register_cached_prelude(store: &mut Store, cached: PreludeCache) {
    // Register the prelude file for file_id → package_id mapping (needed by diagnostics).
    // Items are empty since we're loading definitions from cache.
    use syntax::program::File;
    let mut file = File::new_cached(
        PRELUDE_PACKAGE_ID,
        "prelude.d.lis",
        "prelude.d.lis",
        stdlib::LIS_PRELUDE_SOURCE,
        PRELUDE_FILE_ID,
    );
    file.source_path = deps::prelude_typedef_path();
    store.store_file(file);

    let file_ids: &[u32] = &[PRELUDE_FILE_ID];
    let package = store
        .edit_package(PRELUDE_PACKAGE_ID)
        .expect("prelude package must be registered before loading cached definitions");
    for (qualified_name, cached_definition) in cached.definitions {
        cached_definition.install_into(package.definitions, qualified_name.into(), file_ids);
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
