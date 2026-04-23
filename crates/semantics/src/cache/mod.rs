pub mod go_stdlib;
pub mod prelude;
pub mod types;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use syntax::program::File;

use crate::store::{ENTRY_MODULE_ID, Store};
use types::CachedDefinition;

/// Current cache format version. Bump this when making breaking changes to the cache format.
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// Compiler version hash. Caches from different compiler versions are invalid.
pub const COMPILER_VERSION_HASH: u64 = const_fnv1a_hash(env!("CARGO_PKG_VERSION").as_bytes());

/// Combined stdlib content hash. Changes to any stdlib file (prelude.d.lis
/// or any typedefs/*.d.lis) will change this hash, invalidating all user module caches.
pub const STDLIB_HASH: u64 = stdlib::STDLIB_CONTENT_HASH;

/// Prelude-only content hash (prelude.d.lis).
pub const PRELUDE_HASH: u64 = stdlib::PRELUDE_CONTENT_HASH;

/// Go stdlib-only content hash (typedefs/*.d.lis).
pub const GO_STDLIB_HASH: u64 = stdlib::GO_STD_CONTENT_HASH;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Compile-time FNV-1a hash function for creating version hashes.
const fn const_fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

/// FNV-1a hasher implementing `std::hash::Hasher`.
/// Unlike `DefaultHasher`, this produces deterministic hashes across Rust versions.
struct FnvHasher(u64);

impl FnvHasher {
    fn new() -> Self {
        Self(FNV_OFFSET)
    }
}

impl Hasher for FnvHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= byte as u64;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInterface {
    pub version: u32,

    pub compiler_version: u64,

    pub stdlib_hash: u64,

    /// This module's content hash: hash(source_hash + dependency module_hashes)
    /// Used by downstream modules to detect transitive changes
    pub module_hash: u64,

    pub source_hash: u64,

    /// Module hash of each direct dependency.
    pub dependency_hashes: HashMap<String, u64>,

    pub files: Vec<CachedFile>,

    pub definitions: HashMap<String, CachedDefinition>,

    /// UFCS method pairs for this module, computed during registration.
    pub ufcs_methods: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    pub name: String,
    pub source: String,
}

#[derive(Debug)]
pub struct CompiledModule {
    pub module_id: String,
    pub source_hash: u64,
    pub dep_hashes: HashMap<String, u64>,
}

pub fn hash_module_sources(files: &[File]) -> u64 {
    let mut hasher = FnvHasher::new();

    let mut sorted: Vec<_> = files.iter().collect();
    sorted.sort_by_key(|f| &f.name);

    for file in sorted {
        file.name.hash(&mut hasher);
        file.source.hash(&mut hasher);
    }

    hasher.finish()
}

/// Compute a module's hash from its source hash and dependency hashes.
/// This ensures transitive invalidation: if C changes, B's module_hash changes
/// (even though B's source didn't), which invalidates A's cache.
pub fn compute_module_hash(source_hash: u64, dep_hashes: &HashMap<String, u64>) -> u64 {
    let mut hasher = FnvHasher::new();
    source_hash.hash(&mut hasher);

    let mut deps: Vec<_> = dep_hashes.iter().collect();
    deps.sort_by_key(|(k, _)| *k);
    for (name, hash) in deps {
        name.hash(&mut hasher);
        hash.hash(&mut hasher);
    }

    hasher.finish()
}

pub fn get_dependency_module_hashes(
    module_id: &str,
    edges: &HashMap<String, HashSet<String>>,
    module_hashes: &HashMap<String, u64>,
) -> HashMap<String, u64> {
    let Some(deps) = edges.get(module_id) else {
        return HashMap::default();
    };

    deps.iter()
        .map(|dep_id| {
            let hash = if dep_id.starts_with("go:") || dep_id == "prelude" {
                STDLIB_HASH
            } else {
                *module_hashes.get(dep_id).unwrap_or(&0)
            };
            (dep_id.clone(), hash)
        })
        .collect()
}

pub fn is_cache_valid(
    cache: &ModuleInterface,
    current_source_hash: u64,
    current_dep_hashes: &HashMap<String, u64>,
) -> bool {
    cache.version == CACHE_FORMAT_VERSION
        && cache.compiler_version == COMPILER_VERSION_HASH
        && cache.stdlib_hash == STDLIB_HASH
        && cache.source_hash == current_source_hash
        && cache.dependency_hashes == *current_dep_hashes
}

pub fn cache_path(project_root: &Path, module_id: &str) -> PathBuf {
    project_root
        .join("target")
        .join("cache")
        .join(format!("{}.cache", module_id.replace('/', "_")))
}

pub fn try_load_cache(
    module_id: &str,
    expected_source_hash: u64,
    expected_dep_hashes: &HashMap<String, u64>,
    project_root: &Path,
    check_go_files: bool,
) -> Option<ModuleInterface> {
    let path = cache_path(project_root, module_id);
    let bytes = fs::read(&path).ok()?;
    let interface: ModuleInterface = bincode::deserialize(&bytes).ok()?;

    if !is_cache_valid(&interface, expected_source_hash, expected_dep_hashes) {
        let _ = fs::remove_file(&path);
        return None;
    }

    if check_go_files && !all_go_outputs_exist(module_id, &interface.files, project_root) {
        let _ = fs::remove_file(&path);
        return None;
    }

    Some(interface)
}

fn all_go_outputs_exist(module_id: &str, cached_files: &[CachedFile], project_root: &Path) -> bool {
    let target_dir = if module_id == ENTRY_MODULE_ID {
        project_root.join("target")
    } else {
        project_root.join("target").join(module_id)
    };

    for cached_file in cached_files {
        if cached_file.name.ends_with(".lis") && !cached_file.name.ends_with(".d.lis") {
            let go_name = cached_file.name.replace(".lis", ".go");
            if !target_dir.join(&go_name).exists() {
                return false;
            }
        }
    }

    true
}

pub fn save_module_cache(
    compiled: &CompiledModule,
    store: &Store,
    project_root: &Path,
    ufcs_methods: &HashSet<(String, String)>,
) -> io::Result<()> {
    let module_hash = compute_module_hash(compiled.source_hash, &compiled.dep_hashes);

    let Some(module) = store.get_module(&compiled.module_id) else {
        return Err(io::Error::other("module not found in store"));
    };

    let mut all_files: Vec<_> = module
        .files
        .values()
        .chain(module.typedefs.values())
        .collect();
    all_files.sort_by_key(|f| &f.name);

    let file_id_to_index: HashMap<u32, u32> = all_files
        .iter()
        .enumerate()
        .map(|(idx, f)| (f.id, idx as u32))
        .collect();

    let interface = ModuleInterface {
        version: CACHE_FORMAT_VERSION,
        compiler_version: COMPILER_VERSION_HASH,
        stdlib_hash: STDLIB_HASH,
        module_hash,
        source_hash: compiled.source_hash,
        dependency_hashes: compiled.dep_hashes.clone(),
        files: all_files
            .iter()
            .map(|f| CachedFile {
                name: f.name.clone(),
                source: f.source.clone(),
            })
            .collect(),
        definitions: extract_public_definitions(store, &compiled.module_id, &file_id_to_index),
        ufcs_methods: {
            let prefix = format!("{}.", compiled.module_id);
            ufcs_methods
                .iter()
                .filter(|(type_id, _)| type_id.starts_with(&prefix))
                .cloned()
                .collect()
        },
    };

    let path = cache_path(project_root, &compiled.module_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write to temp file, then rename (atomic)
    let temp_path = path.with_extension("cache.tmp");
    let bytes = bincode::serialize(&interface).map_err(io::Error::other)?;
    fs::write(&temp_path, bytes)?;
    fs::rename(&temp_path, &path)?;

    Ok(())
}

fn extract_public_definitions(
    store: &Store,
    module_id: &str,
    file_id_to_index: &HashMap<u32, u32>,
) -> HashMap<String, CachedDefinition> {
    let Some(module) = store.get_module(module_id) else {
        return HashMap::default();
    };

    module
        .definitions
        .iter()
        .filter(|(_, definition)| definition.visibility().is_public())
        .map(|(name, definition)| {
            (
                name.to_string(),
                CachedDefinition::from_definition(definition, file_id_to_index),
            )
        })
        .collect()
}

/// Register a cached module in the store.
/// This loads the cached definitions and source files without running inference.
pub fn register_cached_module(store: &mut Store, module_id: &str, cached: ModuleInterface) {
    store.add_module(module_id);

    // Clear files stored during module graph construction (parse_module_files stores files
    // eagerly for diagnostic rendering). These have full ASTs but un-inferred typed_patterns,
    // which would cause pattern analysis to panic.
    if let Some(module) = store.get_module_mut(module_id) {
        module.files.clear();
    }

    let mut file_ids: Vec<u32> = vec![];
    for cached_file in &cached.files {
        let file_id = store.new_file_id();
        file_ids.push(file_id);

        let file = File::new_cached(module_id, &cached_file.name, &cached_file.source, file_id);

        store.store_file(module_id, file);
    }

    let module = store.get_module_mut(module_id).unwrap();
    for (qualified_name, cached_definition) in cached.definitions {
        let definition = cached_definition.to_definition(&file_ids);
        module.definitions.insert(qualified_name.into(), definition);
    }

    store.mark_visited(module_id);
}

pub fn is_cache_disabled() -> bool {
    std::env::var("LISETTE_NO_CACHE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::types::{Symbol, Type};

    #[test]
    fn test_hash_module_sources_deterministic() {
        let file1 = File::new_cached("mod", "a.lis", "fn foo() {}", 1);
        let file2 = File::new_cached("mod", "b.lis", "fn bar() {}", 2);

        let hash1 = hash_module_sources(&[file1.clone(), file2.clone()]);
        let hash2 = hash_module_sources(&[file2.clone(), file1.clone()]);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_module_sources_content_sensitive() {
        let file1 = File::new_cached("mod", "a.lis", "fn foo() {}", 1);
        let file2 = File::new_cached("mod", "a.lis", "fn bar() {}", 1);

        let hash1 = hash_module_sources(&[file1]);
        let hash2 = hash_module_sources(&[file2]);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_module_hash_includes_deps() {
        let source_hash = 12345u64;
        let mut deps1 = HashMap::default();
        deps1.insert("dep_a".to_string(), 111u64);

        let mut deps2 = HashMap::default();
        deps2.insert("dep_a".to_string(), 222u64);

        let hash1 = compute_module_hash(source_hash, &deps1);
        let hash2 = compute_module_hash(source_hash, &deps2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_module_hash_deterministic() {
        let source_hash = 12345u64;
        let mut deps = HashMap::default();
        deps.insert("dep_b".to_string(), 222u64);
        deps.insert("dep_a".to_string(), 111u64);

        let hash1 = compute_module_hash(source_hash, &deps);
        let hash2 = compute_module_hash(source_hash, &deps);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_cache_validity_checks_version() {
        let cache = ModuleInterface {
            version: CACHE_FORMAT_VERSION + 1, // Wrong version
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            source_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
        };

        assert!(!is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn test_cache_validity_checks_compiler_version() {
        let cache = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH + 1, // Wrong compiler
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            source_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
        };

        assert!(!is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn test_cache_validity_checks_source_hash() {
        let cache = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            source_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
        };

        assert!(!is_cache_valid(&cache, 200, &HashMap::default()));
        assert!(is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn test_cache_validity_checks_dep_hashes() {
        let mut cached_deps = HashMap::default();
        cached_deps.insert("dep".to_string(), 111u64);

        let cache = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            source_hash: 100,
            dependency_hashes: cached_deps.clone(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
        };

        let mut different_deps = HashMap::default();
        different_deps.insert("dep".to_string(), 222u64);

        assert!(!is_cache_valid(&cache, 100, &different_deps));
        assert!(is_cache_valid(&cache, 100, &cached_deps));
    }

    #[test]
    fn test_type_roundtrip_bincode() {
        let ty = Type::Function {
            params: vec![Type::Nominal {
                id: Symbol::from_raw("int"),
                params: vec![],
                underlying_ty: None,
            }],
            param_mutability: vec![false],
            bounds: vec![],
            return_type: Box::new(Type::Nominal {
                id: Symbol::from_raw("main.MyType"),
                params: vec![Type::Tuple(vec![Type::Never])],
                underlying_ty: None,
            }),
        };

        let bytes = bincode::serialize(&ty).unwrap();
        let restored: Type = bincode::deserialize(&bytes).unwrap();
        assert_eq!(ty, restored);
    }

    #[test]
    fn test_cache_path_format() {
        let path = cache_path(Path::new("/project"), "utils");
        assert_eq!(path, PathBuf::from("/project/target/cache/utils.cache"));

        let path = cache_path(Path::new("/project"), "deep/nested/mod");
        assert_eq!(
            path,
            PathBuf::from("/project/target/cache/deep_nested_mod.cache")
        );
    }

    #[test]
    fn test_get_dependency_module_hashes_uses_stdlib_hash() {
        let mut edges = HashMap::default();
        let mut deps = HashSet::default();
        deps.insert("go:fmt".to_string());
        deps.insert("prelude".to_string());
        deps.insert("user_mod".to_string());
        edges.insert("my_mod".to_string(), deps);

        let mut module_hashes = HashMap::default();
        module_hashes.insert("user_mod".to_string(), 12345u64);

        let result = get_dependency_module_hashes("my_mod", &edges, &module_hashes);

        assert_eq!(result.get("go:fmt"), Some(&STDLIB_HASH));
        assert_eq!(result.get("prelude"), Some(&STDLIB_HASH));
        assert_eq!(result.get("user_mod"), Some(&12345u64));
    }
}
