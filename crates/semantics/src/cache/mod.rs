mod disk;
pub mod go_stdlib;
pub mod prelude;
pub mod types;

use crate::path::DisplayPathBase;
use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use syntax::program::{File, Package, is_test_file};

use crate::loader::is_external_test_package;
use crate::store::{ENTRY_PACKAGE_ID, Store};
use types::CachedDefinition;

/// Current cache format version. Bump this when making breaking changes to the cache format.
pub(super) const CACHE_FORMAT_VERSION: u32 = 1;

/// Compiler version hash. Caches from different compiler versions are invalid.
pub(crate) const COMPILER_VERSION_HASH: u64 =
    const_fnv1a_hash(env!("CARGO_PKG_VERSION").as_bytes());

/// Combined stdlib content hash. Changes to any stdlib file (prelude.d.lis,
/// test_prelude.d.lis, or any typedefs/*.d.lis) will change this hash, invalidating
/// all user package caches.
const STDLIB_HASH: u64 = stdlib::STDLIB_CONTENT_HASH;

/// Prelude content hash (prelude.d.lis + test_prelude.d.lis).
pub(crate) const PRELUDE_HASH: u64 = stdlib::PRELUDE_CONTENT_HASH;

/// Go stdlib-only content hash (typedefs/*.d.lis).
pub(crate) const GO_STDLIB_HASH: u64 = stdlib::GO_STD_CONTENT_HASH;

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
pub struct PackageInterface {
    version: u32,

    compiler_version: u64,

    stdlib_hash: u64,

    /// Hash of all files, tests included; this package's own validity key.
    full_hash: u64,

    /// Package hash of each direct dependency.
    dependency_hashes: HashMap<String, u64>,

    pub(crate) files: Vec<CachedFile>,

    definitions: HashMap<String, CachedDefinition>,

    /// Artifact hash of the on-disk Go files produced for this package.
    /// `None` after a Check-phase save or before the post-write stamp call;
    /// `Some(h)` when the on-disk Go files came from a successful Emit for
    /// artifact hash `h`.
    emit_stamp: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    name: String,
    source: String,
}

#[derive(Debug, Clone)]
pub struct CompiledPackage {
    pub package_id: String,
    pub artifact_hash: u64,
    pub(crate) full_hash: u64,
    pub(crate) dep_hashes: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub struct EmitStamp {
    pub package_id: String,
    pub artifact_hash: u64,
}

/// Hash over the non-sourcemap Go-artifact inputs for one package.
pub fn compute_emit_artifact_hash(production_hash: u64, go_module: &str) -> u64 {
    let mut hasher = FnvHasher::new();
    production_hash.hash(&mut hasher);
    go_module.hash(&mut hasher);
    hasher.finish()
}

/// Hashes a package's sources, given each file's name and source: the
/// production-only hash drives dependents and the emit artifact, the all-files
/// hash drives the package's own validity.
pub(crate) fn hash_package_source_pair<'a>(
    files: impl Iterator<Item = (&'a str, &'a str)> + Clone,
) -> (u64, u64) {
    let production_hash =
        hash_package_sources(files.clone().filter(|(name, _)| !is_test_file(name)));
    let full_hash = if files.clone().any(|(name, _)| is_test_file(name)) {
        hash_package_sources(files)
    } else {
        production_hash
    };
    (production_hash, full_hash)
}

fn hash_package_sources<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> u64 {
    let mut hasher = FnvHasher::new();

    let mut sorted: Vec<(&str, &str)> = files.collect();
    sorted.sort_by_key(|(name, _)| *name);

    for (name, source) in sorted {
        name.hash(&mut hasher);
        source.hash(&mut hasher);
    }

    hasher.finish()
}

/// Compute a package's hash from its production hash and dependency hashes.
/// This ensures transitive invalidation: if C changes, B's package_hash changes
/// (even though B's source didn't), which invalidates A's cache.
pub(crate) fn compute_package_hash(production_hash: u64, dep_hashes: &HashMap<String, u64>) -> u64 {
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

pub(crate) fn get_dependency_package_hashes<'a>(
    dependencies: impl IntoIterator<Item = &'a String>,
    package_hashes: &HashMap<String, u64>,
) -> HashMap<String, u64> {
    dependencies
        .into_iter()
        .map(|dep_id| {
            let hash = if dep_id.starts_with("go:") || dep_id == "prelude" {
                STDLIB_HASH
            } else {
                *package_hashes.get(dep_id).unwrap_or(&0)
            };
            (dep_id.clone(), hash)
        })
        .collect()
}

fn is_cache_valid(
    cache: &PackageInterface,
    current_full_hash: u64,
    current_dep_hashes: &HashMap<String, u64>,
) -> bool {
    cache.version == CACHE_FORMAT_VERSION
        && cache.compiler_version == COMPILER_VERSION_HASH
        && cache.stdlib_hash == STDLIB_HASH
        && cache.full_hash == current_full_hash
        && cache.dependency_hashes == *current_dep_hashes
}

fn cache_path(project_root: &Path, package_id: &str) -> PathBuf {
    project_root
        .join("target")
        .join(".lisette")
        .join("cache")
        .join(cache_file_name(package_id))
}

pub fn cache_file_name(package_id: &str) -> String {
    let mut encoded = String::with_capacity(package_id.len() + 6);
    for ch in package_id.chars() {
        match ch {
            '_' => encoded.push_str("__"),
            '/' => encoded.push_str("_s"),
            _ => encoded.push(ch),
        }
    }
    encoded.push_str(".cache");
    encoded
}

pub(crate) fn try_load_cache(
    package_id: &str,
    expected_full_hash: u64,
    expected_dep_hashes: &HashMap<String, u64>,
    expected_artifact_hash: Option<u64>,
    project_root: &Path,
) -> Option<PackageInterface> {
    let path = cache_path(project_root, package_id);
    let interface: PackageInterface = disk::read(&path).ok()?;

    if !is_cache_valid(&interface, expected_full_hash, expected_dep_hashes) {
        let _ = fs::remove_file(&path);
        return None;
    }

    if let Some(expected_artifact_hash) = expected_artifact_hash {
        if interface.emit_stamp != Some(expected_artifact_hash) {
            return None;
        }
        if !all_go_outputs_exist(package_id, &interface.files, project_root) {
            return None;
        }
    }

    Some(interface)
}

fn all_go_outputs_exist(
    package_id: &str,
    cached_files: &[CachedFile],
    project_root: &Path,
) -> bool {
    let target_dir = if package_id == ENTRY_PACKAGE_ID {
        project_root.join("target")
    } else {
        project_root.join("target").join(package_id)
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

pub fn save_package_cache(
    compiled: &CompiledPackage,
    store: &Store,
    project_root: &Path,
) -> io::Result<()> {
    let Some(package) = store.get_package(&compiled.package_id) else {
        return Err(io::Error::other("package not found in store"));
    };

    let mut all_files: Vec<_> = package.files.values().collect();
    all_files.sort_by_key(|f| &f.name);

    let file_id_to_index: HashMap<u32, u32> = all_files
        .iter()
        .enumerate()
        .map(|(idx, f)| (f.id, idx as u32))
        .collect();

    let interface = PackageInterface {
        version: CACHE_FORMAT_VERSION,
        compiler_version: COMPILER_VERSION_HASH,
        stdlib_hash: STDLIB_HASH,
        full_hash: compiled.full_hash,
        dependency_hashes: compiled.dep_hashes.clone(),
        files: all_files
            .iter()
            .map(|f| CachedFile {
                name: f.name.clone(),
                source: f.source.clone(),
            })
            .collect(),
        definitions: extract_cached_definitions(store, &compiled.package_id, &file_id_to_index),
        emit_stamp: None,
    };

    let path = cache_path(project_root, &compiled.package_id);
    disk::write(&path, &interface)
}

fn extract_cached_definitions(
    store: &Store,
    package_id: &str,
    file_id_to_index: &HashMap<u32, u32>,
) -> HashMap<String, CachedDefinition> {
    let Some(package) = store.get_package(package_id) else {
        return HashMap::default();
    };

    package
        .definitions
        .iter()
        .filter(|(_, definition)| !store.is_test_definition(definition))
        .map(|(name, definition)| {
            (
                name.to_string(),
                CachedDefinition::from_definition(definition, file_id_to_index),
            )
        })
        .collect()
}

pub(crate) fn build_cached_package(
    package_id: String,
    file_id_base: u32,
    cached: PackageInterface,
    src_base: &DisplayPathBase,
    root_base: &DisplayPathBase,
) -> Package {
    let mut package = Package::new(&package_id);
    let mut file_ids: Vec<u32> = Vec::with_capacity(cached.files.len());

    for (index, cached_file) in cached.files.iter().enumerate() {
        let file_id = file_id_base + index as u32;
        file_ids.push(file_id);

        let display_path =
            cached_file_display_path(src_base, root_base, &package_id, &cached_file.name);
        let file = File::new_cached(
            &package_id,
            &cached_file.name,
            &display_path,
            &cached_file.source,
            file_id,
        );
        package.files.insert(file_id, file);
    }

    for (qualified_name, cached_definition) in cached.definitions {
        cached_definition.install_into(&mut package, qualified_name.into(), &file_ids);
    }

    package
}

fn cached_file_display_path(
    src_base: &DisplayPathBase,
    root_base: &DisplayPathBase,
    package_id: &str,
    bare_name: &str,
) -> String {
    if is_external_test_package(package_id) {
        return root_base
            .relative(&Path::new(package_id).join(bare_name))
            .unwrap_or_else(|| bare_name.to_string());
    }
    let rel = if package_id == ENTRY_PACKAGE_ID {
        PathBuf::from(bare_name)
    } else {
        Path::new(package_id).join(bare_name)
    };
    src_base
        .relative(&rel)
        .unwrap_or_else(|| bare_name.to_string())
}

/// Set or clear the `emit_stamp` for each package's cache file. Missing files
/// are skipped; undecodable (e.g. pre-bump) files are unlinked and skipped;
/// other read errors propagate so the sourcemap pre-write clear can hard-fail
/// rather than leave a stale stamp over freshly-overwritten Go.
pub fn apply_emit_stamps(
    project_root: &Path,
    updates: &[(EmitStamp, Option<u64>)],
) -> io::Result<()> {
    for (stamp, value) in updates {
        let path = cache_path(project_root, &stamp.package_id);
        let mut interface: PackageInterface = match disk::read(&path) {
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

pub(crate) fn is_cache_disabled() -> bool {
    std::env::var("LISETTE_NO_CACHE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet as HashSet;
    use syntax::ast::{Annotation, Generic, Span, StructFields};
    use syntax::program::{Attributes, Definition, DefinitionBody, Visibility};
    use syntax::types::{FunctionParameter, Symbol, Type};

    fn generic_struct_definition(visibility: Visibility, file_id: u32) -> Definition {
        let bound_span = Span::new(file_id, 12, 5);
        Definition {
            visibility,
            ty: Type::Nominal {
                id: Symbol::from_raw("package.Box"),
                params: vec![Type::Parameter("T".into())],
            },
            name_span: Some(Span::new(file_id, 0, 3)),
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![Generic::resolved(
                    "T",
                    [(
                        Annotation::Constructor {
                            name: "Comparable".into(),
                            params: Vec::new(),
                            span: bound_span,
                        },
                        Type::Nominal {
                            id: Symbol::from_raw("prelude.Comparable"),
                            params: Vec::new(),
                        },
                    )],
                    Span::new(file_id, 4, 13),
                )],
                fields: StructFields::Record(Vec::new()),
                methods: Default::default(),
                attributes: Attributes::default(),
            },
        }
    }

    #[test]
    fn test_hash_package_sources_deterministic() {
        let first = ("a.lis", "fn foo() {}");
        let second = ("b.lis", "fn bar() {}");

        assert_eq!(
            hash_package_sources([first, second].into_iter()),
            hash_package_sources([second, first].into_iter())
        );
    }

    #[test]
    fn test_hash_package_sources_content_sensitive() {
        assert_ne!(
            hash_package_sources([("a.lis", "fn foo() {}")].into_iter()),
            hash_package_sources([("a.lis", "fn bar() {}")].into_iter())
        );
    }

    #[test]
    fn production_hash_ignores_test_edits_but_full_hash_does_not() {
        let production = ("core.lis", "pub fn add() {}");
        let test_a = ("core.test.lis", "fn t() {}");
        let test_b = ("core.test.lis", "fn t() { add() }");

        let (production_a, full_a) = hash_package_source_pair([production, test_a].into_iter());
        let (production_b, full_b) = hash_package_source_pair([production, test_b].into_iter());
        assert_eq!(
            production_a, production_b,
            "editing a test file must not change the production hash"
        );

        let deps = HashMap::default();
        assert_eq!(
            compute_package_hash(production_a, &deps),
            compute_package_hash(production_b, &deps),
            "the hash propagated to dependents must be invariant to test edits"
        );

        assert_ne!(
            full_a, full_b,
            "editing a test file must change the package's own full hash"
        );
    }

    #[test]
    fn test_compute_package_hash_includes_deps() {
        let source_hash = 12345u64;
        let mut deps1 = HashMap::default();
        deps1.insert("dep_a".to_string(), 111u64);

        let mut deps2 = HashMap::default();
        deps2.insert("dep_a".to_string(), 222u64);

        let hash1 = compute_package_hash(source_hash, &deps1);
        let hash2 = compute_package_hash(source_hash, &deps2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_package_hash_deterministic() {
        let source_hash = 12345u64;
        let mut deps = HashMap::default();
        deps.insert("dep_b".to_string(), 222u64);
        deps.insert("dep_a".to_string(), 111u64);

        let hash1 = compute_package_hash(source_hash, &deps);
        let hash2 = compute_package_hash(source_hash, &deps);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_cache_validity_checks_version() {
        let cache = PackageInterface {
            version: CACHE_FORMAT_VERSION + 1, // Wrong version
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            emit_stamp: None,
        };

        assert!(!is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn test_cache_validity_checks_compiler_version() {
        let cache = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH + 1, // Wrong compiler
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            emit_stamp: None,
        };

        assert!(!is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn test_cache_validity_checks_full_hash() {
        let cache = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            emit_stamp: None,
        };

        assert!(!is_cache_valid(&cache, 200, &HashMap::default()));
        assert!(is_cache_valid(&cache, 100, &HashMap::default()));
    }

    #[test]
    fn build_cached_package_preserves_constant_kind() {
        use syntax::ast::Literal;
        use syntax::program::{Definition, DefinitionBody, ValueKind, Visibility};

        let make_value = |kind| Definition {
            visibility: Visibility::Public,
            ty: Type::Nominal {
                id: Symbol::from_raw("int"),
                params: vec![],
            },
            name_span: None,
            doc: None,
            body: DefinitionBody::Value {
                kind,
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                go_type_param_recipe: None,
            },
        };

        let empty_files = HashMap::default();
        let const_def = make_value(ValueKind::Constant(Literal::Integer {
            value: 5,
            text: None,
        }));
        let declaration_def = make_value(ValueKind::ConstantDeclaration);
        let var_def = make_value(ValueKind::Runtime);

        let mut definitions = HashMap::default();
        definitions.insert(
            "mymod.MAX".to_string(),
            CachedDefinition::from_definition(&const_def, &empty_files),
        );
        definitions.insert(
            "mymod.counter".to_string(),
            CachedDefinition::from_definition(&var_def, &empty_files),
        );
        definitions.insert(
            "mymod.DECL".to_string(),
            CachedDefinition::from_definition(&declaration_def, &empty_files),
        );

        let interface = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 0,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions,
            emit_stamp: None,
        };

        let built = build_cached_package(
            "mymod".to_string(),
            0,
            interface,
            &DisplayPathBase::new(Path::new("/project/src")),
            &DisplayPathBase::new(Path::new("/project")),
        );

        assert!(built.definitions["mymod.MAX"].is_const());
        assert!(built.definitions["mymod.DECL"].is_const());
        assert!(built.definitions["mymod.DECL"].const_value().is_none());
        assert!(!built.definitions["mymod.counter"].is_const());
    }

    #[test]
    fn cached_display_path_roots_external_tests_at_project_root() {
        let src_base = DisplayPathBase::new(Path::new("proj/src"));
        let root_base = DisplayPathBase::new(Path::new("proj"));

        assert_eq!(
            cached_file_display_path(&src_base, &root_base, "tests", "api.test.lis"),
            "proj/tests/api.test.lis"
        );
        assert_eq!(
            cached_file_display_path(&src_base, &root_base, "tests/flows", "flow.test.lis"),
            "proj/tests/flows/flow.test.lis"
        );
        assert_eq!(
            cached_file_display_path(&src_base, &root_base, "geometry", "geometry.lis"),
            "proj/src/geometry/geometry.lis"
        );
        assert_eq!(
            cached_file_display_path(&src_base, &root_base, ENTRY_PACKAGE_ID, "geo.lis"),
            "proj/src/geo.lis"
        );
    }

    #[test]
    fn serialized_attribute_survives_cache_roundtrip() {
        use syntax::ast::StructFields;
        use syntax::program::{Attributes, Definition, DefinitionBody, TypeAttribute, Visibility};

        let mut attributes = Attributes::default();
        attributes.insert(TypeAttribute::Serialized);

        let struct_def = Definition {
            visibility: Visibility::Public,
            ty: Type::Nominal {
                id: Symbol::from_raw("dep.Inner"),
                params: vec![],
            },
            name_span: None,
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![],
                fields: StructFields::Record(vec![]),
                methods: Default::default(),
                attributes,
            },
        };

        let empty_files = HashMap::default();
        let cached = CachedDefinition::from_definition(&struct_def, &empty_files);
        let bytes = bincode::serialize(&cached).unwrap();
        let restored: CachedDefinition = bincode::deserialize(&bytes).unwrap();

        assert!(restored.to_definition(&[]).is_serialized());
    }

    #[test]
    fn cached_definition_preserves_visibility_and_resolved_generic_bounds() {
        let original_file_id = 17;
        let restored_file_id = 91;
        let definition = generic_struct_definition(Visibility::Private, original_file_id);
        let file_map = [(original_file_id, 0)].into_iter().collect();

        let cached = CachedDefinition::from_definition(&definition, &file_map);
        let restored = cached.to_definition(&[restored_file_id]);
        let generics = restored.body.generics().unwrap();
        let generic = &generics[0];

        assert_eq!(
            (
                restored.visibility,
                restored.name_span.unwrap().file_id,
                generic.span.file_id,
                generic.bounds().next().unwrap().get_span().file_id,
                generic.resolved_bounds().unwrap().next().cloned(),
            ),
            (
                Visibility::Private,
                restored_file_id,
                restored_file_id,
                restored_file_id,
                Some(Type::Nominal {
                    id: Symbol::from_raw("prelude.Comparable"),
                    params: Vec::new(),
                }),
            )
        );
    }

    #[test]
    fn test_cache_validity_checks_dep_hashes() {
        let mut cached_deps = HashMap::default();
        cached_deps.insert("dep".to_string(), 111u64);

        let cache = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: cached_deps.clone(),
            files: vec![],
            definitions: HashMap::default(),
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
            vec![FunctionParameter::new(
                Type::Nominal {
                    id: Symbol::from_raw("int"),
                    params: vec![],
                },
                false,
            )],
            vec![],
            Box::new(Type::Nominal {
                id: Symbol::from_raw("main.MyType"),
                params: vec![Type::Tuple(vec![Type::Never])],
            }),
        );

        let bytes = bincode::serialize(&ty).unwrap();
        let restored: Type = bincode::deserialize(&bytes).unwrap();
        assert_eq!(ty, restored);
    }

    #[test]
    fn test_cache_path_format() {
        let path = cache_path(Path::new("/project"), "utils");
        assert_eq!(
            path,
            PathBuf::from("/project/target/.lisette/cache/utils.cache")
        );

        let path = cache_path(Path::new("/project"), "deep/nested/mod");
        assert_eq!(
            path,
            PathBuf::from("/project/target/.lisette/cache/deep_snested_smod.cache")
        );
    }

    #[test]
    fn cache_file_name_is_injective_across_slash_underscore() {
        assert_ne!(cache_file_name("foo/bar"), cache_file_name("foo_bar"));
        assert_ne!(cache_file_name("a_/b"), cache_file_name("a/_b"));
        assert_eq!(cache_file_name("utils"), "utils.cache");
    }

    #[test]
    fn test_get_dependency_package_hashes_uses_stdlib_hash() {
        let mut edges = HashMap::default();
        let mut deps = HashSet::default();
        deps.insert("go:fmt".to_string());
        deps.insert("prelude".to_string());
        deps.insert("user_mod".to_string());
        edges.insert("my_mod".to_string(), deps);

        let mut package_hashes = HashMap::default();
        package_hashes.insert("user_mod".to_string(), 12345u64);

        let result = get_dependency_package_hashes(&edges["my_mod"], &package_hashes);

        assert_eq!(result.get("go:fmt"), Some(&STDLIB_HASH));
        assert_eq!(result.get("prelude"), Some(&STDLIB_HASH));
        assert_eq!(result.get("user_mod"), Some(&12345u64));
    }

    #[test]
    fn hash_package_sources_independent_of_display_path() {
        let cli_file = File::new_cached(
            "greet",
            "greet.lis",
            "src/greet/greet.lis",
            "pub fn x() -> int { 1 }",
            1,
        );
        let lsp_file = File::new_cached(
            "greet",
            "greet.lis",
            "greet.lis",
            "pub fn x() -> int { 1 }",
            1,
        );
        let sources = |file: &File| {
            hash_package_sources([(file.name.as_str(), file.source.as_str())].into_iter())
        };

        assert_eq!(sources(&cli_file), sources(&lsp_file));
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
        std::fs::create_dir_all(root.join("target").join(".lisette").join("cache")).unwrap();

        let interface = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![],
            definitions: HashMap::default(),
            emit_stamp: None,
        };
        let path = cache_path(root, "greet");
        std::fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        let stamp = EmitStamp {
            package_id: "greet".to_string(),
            artifact_hash: 999,
        };
        apply_emit_stamps(root, &[(stamp.clone(), Some(999))]).unwrap();
        let reread: PackageInterface =
            bincode::deserialize(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.emit_stamp, Some(999));
        assert_eq!(reread.full_hash, 100);

        apply_emit_stamps(root, &[(stamp, None)]).unwrap();
        let reread: PackageInterface =
            bincode::deserialize(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.emit_stamp, None);
    }

    #[test]
    fn apply_emit_stamps_missing_cache_is_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let stamp = EmitStamp {
            package_id: "absent".to_string(),
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
            package_id: "corrupt".to_string(),
            artifact_hash: 0,
        };

        let result = apply_emit_stamps(temp.path(), &[(stamp, None)]);

        assert_eq!((result.is_ok(), path.exists()), (true, false));
    }

    #[test]
    fn try_load_cache_rejects_unstamped_for_emit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("target").join(".lisette").join("cache")).unwrap();
        std::fs::create_dir_all(root.join("target").join("greet")).unwrap();
        std::fs::write(root.join("target").join("greet").join("greet.go"), "").unwrap();

        let interface = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![CachedFile {
                name: "greet.lis".to_string(),
                source: String::new(),
            }],
            definitions: HashMap::default(),
            emit_stamp: None,
        };
        let path = cache_path(root, "greet");
        std::fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        let loaded = try_load_cache("greet", 100, &HashMap::default(), None, root);
        assert!(loaded.is_some(), "Check phase must accept unstamped cache");

        let loaded = try_load_cache(
            "greet",
            100,
            &HashMap::default(),
            Some(compute_emit_artifact_hash(100, "github.com/test/x")),
            root,
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
        std::fs::create_dir_all(root.join("target").join(".lisette").join("cache")).unwrap();
        std::fs::create_dir_all(root.join("target").join("greet")).unwrap();
        std::fs::write(root.join("target").join("greet").join("greet.go"), "").unwrap();

        let artifact_hash = compute_emit_artifact_hash(100, "github.com/test/x");

        let interface = PackageInterface {
            version: CACHE_FORMAT_VERSION,
            compiler_version: COMPILER_VERSION_HASH,
            stdlib_hash: STDLIB_HASH,
            full_hash: 100,
            dependency_hashes: HashMap::default(),
            files: vec![CachedFile {
                name: "greet.lis".to_string(),
                source: String::new(),
            }],
            definitions: HashMap::default(),
            emit_stamp: Some(artifact_hash),
        };
        let path = cache_path(root, "greet");
        std::fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        assert!(
            try_load_cache("greet", 100, &HashMap::default(), Some(artifact_hash), root).is_some()
        );

        let stamp = EmitStamp {
            package_id: "greet".to_string(),
            artifact_hash,
        };
        apply_emit_stamps(root, &[(stamp, None)]).unwrap();

        assert!(
            try_load_cache("greet", 100, &HashMap::default(), Some(artifact_hash), root).is_none()
        );
    }
}
