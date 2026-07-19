mod bounds;
mod disk;
pub mod go_stdlib;
pub mod prelude;
pub mod types;

pub(crate) use bounds::restore_cached_generic_bounds;

use crate::path::DisplayPathBase;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use syntax::program::{File, Module};

use crate::store::{ENTRY_MODULE_ID, Store};
use types::CachedDefinition;

/// Current cache format version. Bump this when making breaking changes to the cache format.
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// Compiler version hash. Caches from different compiler versions are invalid.
pub const COMPILER_VERSION_HASH: u64 = const_fnv1a_hash(env!("CARGO_PKG_VERSION").as_bytes());

/// Combined stdlib content hash. Changes to any stdlib file (prelude.d.lis,
/// test_prelude.d.lis, or any typedefs/*.d.lis) will change this hash, invalidating
/// all user module caches.
pub const STDLIB_HASH: u64 = stdlib::STDLIB_CONTENT_HASH;

/// Prelude content hash (prelude.d.lis + test_prelude.d.lis).
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

    /// This module's content hash: hash(production_hash + dependency module_hashes)
    /// Used by downstream modules to detect transitive changes
    pub module_hash: u64,

    /// Hash of production files only; drives `module_hash` and the emit artifact.
    pub production_hash: u64,

    /// Hash of all files, tests included; this module's own validity key.
    pub full_hash: u64,

    /// Module hash of each direct dependency.
    pub dependency_hashes: HashMap<String, u64>,

    pub files: Vec<CachedFile>,

    pub definitions: HashMap<String, CachedDefinition>,

    /// UFCS method pairs for this module, computed during registration.
    pub ufcs_methods: Vec<(String, String)>,

    /// Artifact hash of the on-disk Go files produced for this module.
    /// `None` after a Check-phase save or before the post-write stamp call;
    /// `Some(h)` when the on-disk Go files came from a successful Emit for
    /// artifact hash `h`.
    pub emit_stamp: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub module_id: String,
    /// Production-based hash propagated to dependents (production deps only).
    pub module_hash: u64,
    pub production_hash: u64,
    pub full_hash: u64,
    pub dep_hashes: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub struct EmitStamp {
    pub module_id: String,
    pub artifact_hash: u64,
}

/// Hash over the non-sourcemap Go-artifact inputs for one module.
pub fn compute_emit_artifact_hash(production_hash: u64, go_module: &str) -> u64 {
    let mut hasher = FnvHasher::new();
    production_hash.hash(&mut hasher);
    go_module.hash(&mut hasher);
    hasher.finish()
}

/// Hashes a module's sources: the production-only hash drives dependents and
/// the emit artifact, the all-files hash drives the module's own validity.
pub fn hash_module_source_pair(files: &[File]) -> (u64, u64) {
    let production_hash = hash_module_sources(files.iter().filter(|f| !f.is_test()));
    let full_hash = if files.iter().any(|f| f.is_test()) {
        hash_module_sources(files)
    } else {
        production_hash
    };
    (production_hash, full_hash)
}

pub fn hash_module_sources<'a>(files: impl IntoIterator<Item = &'a File>) -> u64 {
    let mut hasher = FnvHasher::new();

    let mut sorted: Vec<&File> = files.into_iter().collect();
    sorted.sort_by_key(|f| &f.name);

    for file in sorted {
        file.name.hash(&mut hasher);
        file.source.hash(&mut hasher);
    }

    hasher.finish()
}

/// Compute a module's hash from its production hash and dependency hashes.
/// This ensures transitive invalidation: if C changes, B's module_hash changes
/// (even though B's source didn't), which invalidates A's cache.
pub fn compute_module_hash(production_hash: u64, dep_hashes: &HashMap<String, u64>) -> u64 {
    let mut hasher = FnvHasher::new();
    production_hash.hash(&mut hasher);

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
    current_full_hash: u64,
    current_dep_hashes: &HashMap<String, u64>,
) -> bool {
    cache.version == CACHE_FORMAT_VERSION
        && cache.compiler_version == COMPILER_VERSION_HASH
        && cache.stdlib_hash == STDLIB_HASH
        && cache.full_hash == current_full_hash
        && cache.dependency_hashes == *current_dep_hashes
}

pub fn cache_path(project_root: &Path, module_id: &str) -> PathBuf {
    project_root
        .join("target")
        .join("cache")
        .join(cache_file_name(module_id))
}

pub fn cache_file_name(module_id: &str) -> String {
    let mut encoded = String::with_capacity(module_id.len() + 6);
    for ch in module_id.chars() {
        match ch {
            '_' => encoded.push_str("__"),
            '/' => encoded.push_str("_s"),
            _ => encoded.push(ch),
        }
    }
    encoded.push_str(".cache");
    encoded
}

pub fn try_load_cache(
    module_id: &str,
    expected_full_hash: u64,
    expected_dep_hashes: &HashMap<String, u64>,
    expected_artifact_hash: Option<u64>,
    project_root: &Path,
    check_go_files: bool,
) -> Option<ModuleInterface> {
    let path = cache_path(project_root, module_id);
    let interface: ModuleInterface = disk::read(&path).ok()?;

    if !is_cache_valid(&interface, expected_full_hash, expected_dep_hashes) {
        let _ = fs::remove_file(&path);
        return None;
    }

    if check_go_files {
        if interface.emit_stamp != expected_artifact_hash {
            return None;
        }
        if !all_go_outputs_exist(module_id, &interface.files, project_root) {
            return None;
        }
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
        if cached_file.name.ends_with(".lis")
            && !cached_file.name.ends_with(".d.lis")
            && !cached_file.name.ends_with(".test.lis")
        {
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
    let module_hash = compiled.module_hash;

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
        production_hash: compiled.production_hash,
        full_hash: compiled.full_hash,
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
        emit_stamp: None,
    };

    let path = cache_path(project_root, &compiled.module_id);
    disk::write(&path, &interface)
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
        .filter(|(_, definition)| !store.is_test_definition(definition))
        .map(|(name, definition)| {
            let is_const = module.const_names.contains(name);
            (
                name.to_string(),
                CachedDefinition::from_definition(definition, is_const, file_id_to_index),
            )
        })
        .collect()
}

pub(crate) struct CachedModuleBuild {
    pub module_id: String,
    pub module: Module,
    /// `(file_id, module_id)` pairs for the store's file -> module index.
    pub file_map: Vec<(u32, String)>,
    pub ufcs_methods: Vec<(String, String)>,
}

pub(crate) fn build_cached_module(
    module_id: String,
    file_id_base: u32,
    cached: ModuleInterface,
    display_base: &DisplayPathBase,
) -> CachedModuleBuild {
    let mut module = Module::new(&module_id);
    let mut file_ids: Vec<u32> = Vec::with_capacity(cached.files.len());
    let mut file_map: Vec<(u32, String)> = Vec::with_capacity(cached.files.len());

    for (index, cached_file) in cached.files.iter().enumerate() {
        let file_id = file_id_base + index as u32;
        file_ids.push(file_id);
        file_map.push((file_id, module_id.clone()));

        let display_path = cached_file_display_path(display_base, &module_id, &cached_file.name);
        let file = File::new_cached(
            &module_id,
            &cached_file.name,
            &display_path,
            &cached_file.source,
            file_id,
        );
        if file.is_d_lis() {
            module.typedefs.insert(file_id, file);
        } else {
            module.files.insert(file_id, file);
        }
    }

    for (qualified_name, cached_definition) in cached.definitions {
        cached_definition.install_into(&mut module, qualified_name.into(), &file_ids);
    }

    CachedModuleBuild {
        module_id,
        module,
        file_map,
        ufcs_methods: cached.ufcs_methods,
    }
}

fn cached_file_display_path(
    display_base: &DisplayPathBase,
    module_id: &str,
    bare_name: &str,
) -> String {
    let rel = if module_id == ENTRY_MODULE_ID {
        PathBuf::from(bare_name)
    } else {
        Path::new(module_id).join(bare_name)
    };
    display_base
        .relative(&rel)
        .unwrap_or_else(|| bare_name.to_string())
}

/// Set or clear the `emit_stamp` for each module's cache file. Missing files
/// are skipped; undecodable (e.g. pre-bump) files are unlinked and skipped;
/// other read errors propagate so the sourcemap pre-write clear can hard-fail
/// rather than leave a stale stamp over freshly-overwritten Go.
pub fn apply_emit_stamps(
    project_root: &Path,
    updates: &[(EmitStamp, Option<u64>)],
) -> io::Result<()> {
    for (stamp, value) in updates {
        let path = cache_path(project_root, &stamp.module_id);
        let mut interface: ModuleInterface = match disk::read(&path) {
            Ok(interface) => interface,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::InvalidData
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        interface.emit_stamp = *value;
        disk::write(&path, &interface)?;
    }
    Ok(())
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
        let file1 = File::new_cached("mod", "a.lis", "a.lis", "fn foo() {}", 1);
        let file2 = File::new_cached("mod", "b.lis", "b.lis", "fn bar() {}", 2);

        let hash1 = hash_module_sources(&[file1.clone(), file2.clone()]);
        let hash2 = hash_module_sources(&[file2.clone(), file1.clone()]);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_module_sources_content_sensitive() {
        let file1 = File::new_cached("mod", "a.lis", "a.lis", "fn foo() {}", 1);
        let file2 = File::new_cached("mod", "a.lis", "a.lis", "fn bar() {}", 1);

        let hash1 = hash_module_sources(&[file1]);
        let hash2 = hash_module_sources(&[file2]);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn production_hash_ignores_test_edits_but_full_hash_does_not() {
        let prod = File::new_cached("math", "core.lis", "core.lis", "pub fn add() {}", 1);
        let test_a = File::new_cached("math", "core.test.lis", "core.test.lis", "fn t() {}", 2);
        let test_b = File::new_cached(
            "math",
            "core.test.lis",
            "core.test.lis",
            "fn t() { add() }",
            2,
        );

        let production_a =
            hash_module_sources([&prod, &test_a].into_iter().filter(|f| !f.is_test()));
        let production_b =
            hash_module_sources([&prod, &test_b].into_iter().filter(|f| !f.is_test()));
        assert_eq!(
            production_a, production_b,
            "editing a test file must not change the production hash"
        );

        let deps = HashMap::default();
        assert_eq!(
            compute_module_hash(production_a, &deps),
            compute_module_hash(production_b, &deps),
            "the hash propagated to dependents must be invariant to test edits"
        );

        let full_a = hash_module_sources([&prod, &test_a]);
        let full_b = hash_module_sources([&prod, &test_b]);
        assert_ne!(
            full_a, full_b,
            "editing a test file must change the module's own full hash"
        );
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
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: None,
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
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: None,
        };

        assert!(!is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn test_cache_validity_checks_full_hash() {
        let cache = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: None,
        };

        assert!(!is_cache_valid(&cache, 200, &HashMap::default()));
        assert!(is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn build_cached_module_restores_const_names() {
        use syntax::ast::Literal;
        use syntax::program::{Definition, DefinitionBody, Visibility};

        let make_value = |const_value: Option<Literal>| Definition {
            visibility: Visibility::Public,
            ty: Type::Nominal {
                id: Symbol::from_raw("int"),
                params: vec![],
                underlying_ty: None,
            },
            name: None,
            name_span: None,
            doc: None,
            body: DefinitionBody::Value {
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                go_type_param_recipe: None,
                const_value,
            },
        };

        let empty_files = HashMap::default();
        let const_def = make_value(Some(Literal::Integer {
            value: 5,
            text: None,
        }));
        let var_def = make_value(None);

        let mut definitions = HashMap::default();
        definitions.insert(
            "mymod.MAX".to_string(),
            CachedDefinition::from_definition(&const_def, true, &empty_files),
        );
        definitions.insert(
            "mymod.counter".to_string(),
            CachedDefinition::from_definition(&var_def, false, &empty_files),
        );

        let interface = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            production_hash: 0,
            full_hash: 0,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions,
            ufcs_methods: vec![],
            emit_stamp: None,
        };

        let built = build_cached_module(
            "mymod".to_string(),
            0,
            interface,
            &DisplayPathBase::new(Path::new("/project/src")),
        );

        assert!(built.module.const_names.contains("mymod.MAX"));
        assert!(!built.module.const_names.contains("mymod.counter"));
    }

    #[test]
    fn serialized_attribute_survives_cache_roundtrip() {
        use syntax::ast::StructKind;
        use syntax::program::{Attributes, Definition, DefinitionBody, TypeAttribute, Visibility};

        let mut attributes = Attributes::default();
        attributes.insert(TypeAttribute::Serialized, ());

        let struct_def = Definition {
            visibility: Visibility::Public,
            ty: Type::Nominal {
                id: Symbol::from_raw("dep.Inner"),
                params: vec![],
                underlying_ty: None,
            },
            name: Some("Inner".into()),
            name_span: None,
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![],
                fields: vec![],
                kind: StructKind::Record,
                methods: Default::default(),
                constructor: None,
                attributes,
            },
        };

        let empty_files = HashMap::default();
        let cached = CachedDefinition::from_definition(&struct_def, false, &empty_files);
        let bytes = bincode::serialize(&cached).unwrap();
        let restored: CachedDefinition = bincode::deserialize(&bytes).unwrap();

        assert!(restored.to_definition(&[]).is_serialized());
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
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: cached_deps.clone(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: None,
        };

        let mut different_deps = HashMap::default();
        different_deps.insert("dep".to_string(), 222u64);

        assert!(!is_cache_valid(&cache, 100, &different_deps));
        assert!(is_cache_valid(&cache, 100, &cached_deps));
    }

    #[test]
    fn test_type_roundtrip_bincode() {
        let ty = Type::function(
            vec![Type::Nominal {
                id: Symbol::from_raw("int"),
                params: vec![],
                underlying_ty: None,
            }],
            vec![false],
            vec![],
            Box::new(Type::Nominal {
                id: Symbol::from_raw("main.MyType"),
                params: vec![Type::Tuple(vec![Type::Never])],
                underlying_ty: None,
            }),
        );

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
            PathBuf::from("/project/target/cache/deep_snested_smod.cache")
        );
    }

    #[test]
    fn cache_file_name_is_injective_across_slash_underscore() {
        assert_ne!(cache_file_name("foo/bar"), cache_file_name("foo_bar"));
        assert_ne!(cache_file_name("a_/b"), cache_file_name("a/_b"));
        assert_eq!(cache_file_name("utils"), "utils.cache");
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

    #[test]
    fn hash_module_sources_independent_of_display_path() {
        let cli_file = File::new(
            "greet",
            "greet.lis",
            "src/greet/greet.lis",
            "pub fn x() -> int { 1 }",
            vec![],
            None,
            1,
        );
        let lsp_file = File::new(
            "greet",
            "greet.lis",
            "greet.lis",
            "pub fn x() -> int { 1 }",
            vec![],
            None,
            1,
        );

        assert_eq!(
            hash_module_sources(&[cli_file]),
            hash_module_sources(&[lsp_file]),
        );
    }

    #[test]
    fn cache_file_purity_no_src_prefix() {
        let cached = CachedFile {
            name: "greet.lis".to_string(),
            source: "pub fn x() -> int { 1 }".to_string(),
        };
        let bytes = bincode::serialize(&cached).unwrap();
        let serialized = String::from_utf8_lossy(&bytes);
        assert!(
            !serialized.contains("src/"),
            "CachedFile must not contain `src/` prefix; got: {serialized:?}"
        );
    }

    #[test]
    fn artifact_hash_depends_on_go_module() {
        let h1 = compute_emit_artifact_hash(100, "github.com/old/proj");
        let h2 = compute_emit_artifact_hash(100, "github.com/new/proj");
        assert_ne!(h1, h2);
    }

    #[test]
    fn apply_emit_stamps_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("target").join("cache")).unwrap();

        let interface = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: None,
        };
        let path = cache_path(root, "greet");
        std::fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        let stamp = EmitStamp {
            module_id: "greet".to_string(),
            artifact_hash: 999,
        };
        apply_emit_stamps(root, &[(stamp.clone(), Some(999))]).unwrap();
        let reread: ModuleInterface = bincode::deserialize(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.emit_stamp, Some(999));
        assert_eq!(reread.full_hash, 100);

        apply_emit_stamps(root, &[(stamp, None)]).unwrap();
        let reread: ModuleInterface = bincode::deserialize(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.emit_stamp, None);
    }

    #[test]
    fn apply_emit_stamps_missing_cache_is_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let stamp = EmitStamp {
            module_id: "absent".to_string(),
            artifact_hash: 0,
        };
        let result = apply_emit_stamps(tmp.path(), &[(stamp, None)]);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_emit_stamps_removes_corrupt_cache() {
        let temp = tempfile::tempdir().unwrap();
        let path = cache_path(temp.path(), "corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"invalid").unwrap();
        let stamp = EmitStamp {
            module_id: "corrupt".to_string(),
            artifact_hash: 0,
        };

        let result = apply_emit_stamps(temp.path(), &[(stamp, None)]);

        assert_eq!((result.is_ok(), path.exists()), (true, false));
    }

    #[test]
    fn try_load_cache_rejects_unstamped_for_emit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("target").join("cache")).unwrap();
        std::fs::create_dir_all(root.join("target").join("greet")).unwrap();
        std::fs::write(root.join("target").join("greet").join("greet.go"), "").unwrap();

        let interface = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![CachedFile {
                name: "greet.lis".to_string(),
                source: String::new(),
            }],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: None,
        };
        let path = cache_path(root, "greet");
        std::fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        let loaded = try_load_cache("greet", 100, &HashMap::default(), None, root, false);
        assert!(loaded.is_some(), "Check phase must accept unstamped cache");

        let loaded = try_load_cache(
            "greet",
            100,
            &HashMap::default(),
            Some(compute_emit_artifact_hash(100, "github.com/test/x")),
            root,
            true,
        );
        assert!(
            loaded.is_none(),
            "Emit phase must reject cache with emit_stamp = None"
        );
    }

    #[test]
    fn try_load_cache_rejects_after_sourcemap_invalidation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("target").join("cache")).unwrap();
        std::fs::create_dir_all(root.join("target").join("greet")).unwrap();
        std::fs::write(root.join("target").join("greet").join("greet.go"), "").unwrap();

        let artifact_hash = compute_emit_artifact_hash(100, "github.com/test/x");

        let interface = ModuleInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            module_hash: 0,
            production_hash: 100,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![CachedFile {
                name: "greet.lis".to_string(),
                source: String::new(),
            }],
            definitions: HashMap::default(),
            ufcs_methods: vec![],
            emit_stamp: Some(artifact_hash),
        };
        let path = cache_path(root, "greet");
        std::fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        assert!(
            try_load_cache(
                "greet",
                100,
                &HashMap::default(),
                Some(artifact_hash),
                root,
                true,
            )
            .is_some()
        );

        let stamp = EmitStamp {
            module_id: "greet".to_string(),
            artifact_hash,
        };
        apply_emit_stamps(root, &[(stamp, None)]).unwrap();

        assert!(
            try_load_cache(
                "greet",
                100,
                &HashMap::default(),
                Some(artifact_hash),
                root,
                true,
            )
            .is_none()
        );
    }
}
