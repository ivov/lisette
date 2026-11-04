use rustc_hash::FxHashMap as HashMap;
use std::ops::Range;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use stdlib::{Target, get_go_stdlib_typedef};

use super::disk;
use super::types::CachedDefinition;
use super::{CACHE_FORMAT_VERSION, COMPILER_VERSION_HASH, GO_STDLIB_HASH};
use crate::store::Store;
use std::fs;
use syntax::program::File;

#[derive(Serialize, Deserialize)]
struct GoStdlibHeader {
    version: u32,
    content_hash: u64,
    compiler_version: u64,
    block_ranges: HashMap<String, (usize, usize)>,
}

pub struct GoStdlibCache {
    packages: HashMap<String, Range<usize>>,
    bytes: Vec<u8>,
}

impl GoStdlibCache {
    pub fn package_ids(&self) -> impl Iterator<Item = &String> {
        self.packages.keys()
    }

    fn block(&self, package_id: &str) -> Option<&[u8]> {
        let range = self.packages.get(package_id)?;
        self.bytes.get(range.clone())
    }

    pub fn decode(&self, package_id: &str) -> Option<GoPackageCache> {
        bincode::deserialize(self.block(package_id)?).ok()
    }

    fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        let header_length = usize::try_from(u64::from_le_bytes(
            bytes.get(..HEADER_LENGTH_SIZE)?.try_into().ok()?,
        ))
        .ok()?;
        let blocks_start = HEADER_LENGTH_SIZE.checked_add(header_length)?;
        let header: GoStdlibHeader =
            bincode::deserialize(bytes.get(HEADER_LENGTH_SIZE..blocks_start)?).ok()?;
        if header.version != CACHE_FORMAT_VERSION
            || header.content_hash != GO_STDLIB_HASH
            || header.compiler_version != COMPILER_VERSION_HASH
        {
            return None;
        }
        let packages = header
            .block_ranges
            .into_iter()
            .map(|(package_id, (start, end))| {
                (package_id, blocks_start + start..blocks_start + end)
            })
            .collect();
        Some(Self { packages, bytes })
    }

    fn encode(blocks: &[(&str, &[u8])]) -> Option<Vec<u8>> {
        let mut block_ranges = HashMap::default();
        let mut offset = 0;
        for (package_id, block) in blocks {
            block_ranges.insert(package_id.to_string(), (offset, offset + block.len()));
            offset += block.len();
        }
        let header = bincode::serialize(&GoStdlibHeader {
            version: CACHE_FORMAT_VERSION,
            content_hash: GO_STDLIB_HASH,
            compiler_version: COMPILER_VERSION_HASH,
            block_ranges,
        })
        .ok()?;
        let mut bytes = Vec::with_capacity(HEADER_LENGTH_SIZE + header.len() + offset);
        bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&header);
        for (_, block) in blocks {
            bytes.extend_from_slice(block);
        }
        Some(bytes)
    }
}

const HEADER_LENGTH_SIZE: usize = size_of::<u64>();

#[derive(Serialize, Deserialize)]
pub struct GoPackageCache {
    pub definitions: HashMap<String, CachedDefinition>,
    /// Go package imports (e.g., `["go:io", "go:sync"]`).
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
    let bytes = fs::read(&path).ok()?;
    let cache = GoStdlibCache::from_bytes(bytes);
    if cache.is_none() {
        let _ = fs::remove_file(&path);
    }
    cache
}

pub(crate) fn save_go_stdlib_cache(
    store: &Store,
    go_package_ids: &[String],
    previous: Option<GoStdlibCache>,
    target: Target,
) {
    let Some(path) = cache_path(target) else {
        return;
    };

    // Go definitions don't reference files, so file_id_to_index is always empty.
    let empty_file_map = HashMap::default();
    let mut encoded: Vec<(&str, Vec<u8>)> = Vec::with_capacity(go_package_ids.len());
    for package_id in go_package_ids {
        let Some(package) = store.get_package(package_id) else {
            continue;
        };
        let definitions: HashMap<String, CachedDefinition> = package
            .definitions
            .iter()
            .map(|(name, definition)| {
                (
                    name.to_string(),
                    CachedDefinition::from_definition(definition, &empty_file_map),
                )
            })
            .collect();

        let go_imports = get_go_imports_from_source(package_id, target);

        let Ok(block) = bincode::serialize(&GoPackageCache {
            definitions,
            go_imports,
        }) else {
            return;
        };
        encoded.push((package_id, block));
    }

    let mut blocks: Vec<(&str, &[u8])> = encoded
        .iter()
        .map(|(package_id, block)| (*package_id, block.as_slice()))
        .collect();
    if let Some(previous) = &previous {
        for package_id in previous.package_ids() {
            if !go_package_ids.contains(package_id)
                && let Some(block) = previous.block(package_id)
            {
                blocks.push((package_id, block));
            }
        }
    }

    if let Some(bytes) = GoStdlibCache::encode(&blocks) {
        disk::write_global_bytes(&path, &bytes, "stdlib_defs");
    }
}

/// Load a Go package and its transitive deps from cache, recursively.
pub(crate) fn load_cached_go_package(
    store: &mut Store,
    package_id: &str,
    cache: &GoStdlibCache,
    target: Target,
) {
    if store.has(package_id) {
        return;
    }

    let Some(cached) = cache.decode(package_id) else {
        return;
    };

    // Presence is the recursion guard as well as the canonical loaded-package fact.
    store.add_package(package_id);

    // Load transitive deps first
    let imports = cached.go_imports.clone();
    for dep in &imports {
        load_cached_go_package(store, dep, cache, target);
    }

    register_cached_go_package(store, package_id, &cached, target);
}

fn register_cached_go_package(
    store: &mut Store,
    package_id: &str,
    cached: &GoPackageCache,
    target: Target,
) {
    let go_pkg = package_id.strip_prefix("go:");
    let source = go_pkg.and_then(|go_pkg| get_go_stdlib_typedef(go_pkg, target));

    if let Some(source) = source
        && let Some(pkg_name) = stdlib::declared_package_name(source)
    {
        store
            .go_package_names
            .insert(package_id.to_string(), pkg_name.to_string());
    }

    // Register the typedef File and its on-disk path so go-to-definition can
    // navigate. The files are written by the LSP at startup.
    let owned_file_id;
    let mut file_ids: &[u32] = &[];
    if let (Some(go_pkg), Some(source)) = (go_pkg, source) {
        let file_id = store.new_file_id();
        let filename = format!("{}.d.lis", go_pkg.replace('/', "_"));
        let mut file = File::new_cached(package_id, &filename, &filename, source, file_id);
        file.source_path = deps::stdlib_typedef_path(target, go_pkg);
        store.store_file(file);
        owned_file_id = [file_id];
        file_ids = &owned_file_id;
    }

    let package = store.get_package_mut(package_id).unwrap();
    for (qualified_name, cached_definition) in &cached.definitions {
        cached_definition.install_into(package, qualified_name.clone().into(), file_ids);
    }
}

/// Extract Go imports from a package's `.d.lis` source without parsing.
fn get_go_imports_from_source(package_id: &str, target: Target) -> Vec<String> {
    let Some(go_pkg) = package_id.strip_prefix("go:") else {
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
    fn package_presence_terminates_cached_import_cycles() {
        let package = |import: &str| GoPackageCache {
            definitions: Default::default(),
            go_imports: vec![import.to_string()],
        };
        let a = bincode::serialize(&package("go:example/b")).unwrap();
        let b = bincode::serialize(&package("go:example/a")).unwrap();
        let bytes = GoStdlibCache::encode(&[
            ("go:example/a", a.as_slice()),
            ("go:example/b", b.as_slice()),
        ])
        .unwrap();
        let cache = GoStdlibCache::from_bytes(bytes).unwrap();
        let mut store = Store::new();

        load_cached_go_package(&mut store, "go:example/a", &cache, Target::host());

        assert!(store.has("go:example/a"));
        assert!(store.has("go:example/b"));
    }
}
