mod disk;
pub mod go_stdlib;
pub mod prelude;
pub mod types;

use crate::path::DisplayPathBase;
use crate::{CompilePhase, ProjectKind};
use diagnostics::LisetteDiagnostic;
use rustc_hash::FxHashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::mem;
use std::path::{Path, PathBuf};
use syntax::ast::Span;
use syntax::program::{GoMethodPolicy, PRELUDE_PACKAGE_ID, go_filename, is_go_import};

use serde::{Deserialize, Serialize};
use stdlib::Target;
use syntax::program::{File, Package, SourceFileKind, is_test_file};

use crate::loader::is_external_test_package;
use crate::store::{ENTRY_PACKAGE_ID, Store};
use std::env;
use syntax::types::Symbol;
use types::CachedDefinition;

/// Current cache format version. Bump this when making breaking changes to the cache format.
pub(super) const CACHE_FORMAT_VERSION: u32 = 1;

/// The package version plus the compiler crate sources, so two builds of one version differ.
pub(crate) const COMPILER_BUILD_HASH: u64 =
    include!(concat!(env!("OUT_DIR"), "/compiler_build_hash.rs"));

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    format_version: u32,
    compiler_build: u64,
    stdlib: u64,
    target: String,
    sources: u64,
    dependencies: FxHashMap<String, u64>,
    /// All declared Go modules, because a typedef can reach typedefs that load later.
    third_party_typedefs: String,
    diagnostic_mode: DiagnosticMode,
}

pub(crate) struct CacheKeyInputs {
    pub(crate) target: Target,
    pub(crate) sources: u64,
    pub(crate) dependencies: FxHashMap<String, u64>,
    pub(crate) third_party_typedefs: String,
    pub(crate) diagnostic_mode: DiagnosticMode,
}

impl CacheKey {
    pub(crate) fn new(inputs: CacheKeyInputs) -> Self {
        let CacheKeyInputs {
            target,
            sources,
            dependencies,
            third_party_typedefs,
            diagnostic_mode,
        } = inputs;
        Self {
            format_version: CACHE_FORMAT_VERSION,
            compiler_build: COMPILER_BUILD_HASH,
            stdlib: STDLIB_HASH,
            target: target.cache_segment(),
            sources,
            dependencies,
            third_party_typedefs,
            diagnostic_mode,
        }
    }

    fn differs_only_in_mode(&self, other: &Self) -> bool {
        self.diagnostic_mode != other.diagnostic_mode
            && Self {
                diagnostic_mode: other.diagnostic_mode,
                ..self.clone()
            } == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInterface {
    key: CacheKey,

    diagnostics: Vec<LisetteDiagnostic>,

    pub(crate) files: Vec<CachedFile>,

    definitions: FxHashMap<String, CachedDefinition>,

    /// Artifact hash of the on-disk Go files produced for this package.
    /// `None` after a Check-phase save or before the post-write stamp call;
    /// `Some(h)` when the on-disk Go files came from a successful Emit for
    /// artifact hash `h`.
    pub(crate) emit_stamp: Option<u64>,

    pub(crate) go_method_context: u64,
}

impl PackageInterface {
    pub(crate) fn take_diagnostics(&mut self, file_id_base: u32) -> Vec<LisetteDiagnostic> {
        mem::take(&mut self.diagnostics)
            .into_iter()
            .filter_map(|diagnostic| {
                diagnostic.try_map_spans(|span| {
                    Some(Span {
                        file_id: file_id_base.checked_add(span.file_id)?,
                        ..span
                    })
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    name: String,
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticMode {
    include_tests: bool,
    library: bool,
}

impl DiagnosticMode {
    pub fn new(phase: CompilePhase, kind: ProjectKind) -> Self {
        Self {
            include_tests: phase.includes_tests(),
            library: kind == ProjectKind::Library,
        }
    }
}

#[cfg(test)]
impl Default for DiagnosticMode {
    fn default() -> Self {
        Self::new(CompilePhase::Check, ProjectKind::Binary)
    }
}

#[derive(Debug, Clone)]
pub struct CompiledPackage {
    pub package_id: String,
    pub artifact_hash: u64,
    pub(crate) key: CacheKey,
    pub(crate) go_method_context: u64,
}

#[derive(Debug, Clone)]
pub struct EmitStamp {
    pub package_id: String,
    pub artifact_hash: u64,
}

/// Hash over the non-sourcemap Go-artifact inputs for one package.
pub fn compute_emit_artifact_hash(production_hash: u64, go_module: &str, context: u64) -> u64 {
    let mut hasher = FnvHasher::new();
    production_hash.hash(&mut hasher);
    go_module.hash(&mut hasher);
    let input_hash = hasher.finish();
    let mut hasher = FnvHasher::new();
    input_hash.hash(&mut hasher);
    context.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn global_emit_context(policy: &GoMethodPolicy) -> u64 {
    let mut hasher = FnvHasher::new();
    policy.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn go_method_check_context(policy: &GoMethodPolicy) -> u64 {
    let mut hasher = FnvHasher::new();
    policy.exported_method_names.hash(&mut hasher);
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
pub(crate) fn compute_package_hash(
    production_hash: u64,
    dep_hashes: &FxHashMap<String, u64>,
) -> u64 {
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

pub(crate) fn get_dependency_package_hashes<'a, T: AsRef<str> + ?Sized + 'a>(
    dependencies: impl IntoIterator<Item = &'a T>,
    package_hashes: &FxHashMap<String, u64>,
) -> FxHashMap<String, u64> {
    dependencies
        .into_iter()
        .map(|dep_id| {
            let dep_id = dep_id.as_ref();
            let hash = if is_go_import(dep_id) || dep_id == PRELUDE_PACKAGE_ID {
                STDLIB_HASH
            } else {
                *package_hashes.get(dep_id).unwrap_or(&0)
            };
            (dep_id.to_string(), hash)
        })
        .collect()
}

fn cache_dir(project_root: &Path, target: Target) -> PathBuf {
    project_root
        .join("target")
        .join(".lisette")
        .join("cache")
        .join(target.cache_segment())
}

fn cache_path(project_root: &Path, package_id: &str, target: Target) -> PathBuf {
    cache_dir(project_root, target).join(cache_file_name(package_id))
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
    compiled: &CompiledPackage,
    project_root: &Path,
    target: Target,
) -> Option<PackageInterface> {
    let path = cache_path(project_root, &compiled.package_id, target);
    let interface: PackageInterface = disk::read(&path).ok()?;

    let differs_only_in_mode = interface.key.differs_only_in_mode(&compiled.key);
    let mode_independent = differs_only_in_mode
        && interface.key.diagnostic_mode.library == compiled.key.diagnostic_mode.library
        && interface.files.iter().all(|file| !is_test_file(&file.name))
        && interface
            .definitions
            .values()
            .all(|definition| definition.as_definition().visibility.is_public());
    if interface.key != compiled.key && !mode_independent {
        if !differs_only_in_mode {
            let _ = fs::remove_file(&path);
        }
        return None;
    }

    Some(interface)
}

pub(crate) fn all_go_outputs_exist(
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
        if SourceFileKind::of(&cached_file.name).emits_go() {
            let go_name = go_filename(&cached_file.name);
            if !target_dir.join(go_name).exists() {
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
    target: Target,
    diagnostics: &[&LisetteDiagnostic],
) -> io::Result<()> {
    let Some(package) = store.get_package(&compiled.package_id) else {
        return Err(io::Error::other("package not found in store"));
    };

    let mut all_files: Vec<_> = package.files.values().collect();
    all_files.sort_by_key(|f| &f.name);

    let file_id_to_index: FxHashMap<u32, u32> = all_files
        .iter()
        .enumerate()
        .map(|(idx, f)| (f.id, idx as u32))
        .collect();

    let Some(diagnostics) = diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic.try_map_spans(|span| {
                Some(Span {
                    file_id: *file_id_to_index.get(&span.file_id)?,
                    ..span
                })
            })
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(());
    };

    let interface = PackageInterface {
        key: compiled.key.clone(),
        diagnostics,
        files: all_files
            .iter()
            .map(|f| CachedFile {
                name: f.name.clone(),
                source: f.source.clone(),
            })
            .collect(),
        definitions: extract_cached_definitions(store, &compiled.package_id, &file_id_to_index),
        emit_stamp: None,
        go_method_context: compiled.go_method_context,
    };

    let path = cache_path(project_root, &compiled.package_id, target);
    disk::write(&path, &interface)
}

fn extract_cached_definitions(
    store: &Store,
    package_id: &str,
    file_id_to_index: &FxHashMap<u32, u32>,
) -> FxHashMap<String, CachedDefinition> {
    let Some(package) = store.get_package(package_id) else {
        return FxHashMap::default();
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
        cached_definition.install_into(
            &mut package.definitions,
            Symbol::from_raw(qualified_name),
            &file_ids,
        );
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
            .display_path(&Path::new(package_id).join(bare_name))
            .unwrap_or_else(|| bare_name.to_string());
    }
    let rel = if package_id == ENTRY_PACKAGE_ID {
        PathBuf::from(bare_name)
    } else {
        Path::new(package_id).join(bare_name)
    };
    src_base
        .display_path(&rel)
        .unwrap_or_else(|| bare_name.to_string())
}

/// Set or clear the `emit_stamp` for each package's cache file. Missing files
/// are skipped; undecodable (e.g. pre-bump) files are unlinked and skipped;
/// other read errors propagate so the sourcemap pre-write clear can hard-fail
/// rather than leave a stale stamp over freshly-overwritten Go.
pub fn apply_emit_stamps(
    project_root: &Path,
    updates: &[(EmitStamp, Option<u64>)],
    target: Target,
) -> io::Result<()> {
    for (stamp, value) in updates {
        let path = cache_path(project_root, &stamp.package_id, target);
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

pub fn discard_package_cache(
    project_root: &Path,
    package_id: &str,
    target: Target,
) -> io::Result<()> {
    match fs::remove_file(cache_path(project_root, package_id, target)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn clear_emit_stamps(project_root: &Path, target: Target) -> io::Result<()> {
    let entries = match fs::read_dir(cache_dir(project_root, target)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "cache") {
            continue;
        }
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
        if interface.emit_stamp.is_none() {
            continue;
        }
        interface.emit_stamp = None;
        disk::write(&path, &interface)?;
    }
    Ok(())
}

pub(crate) fn is_cache_disabled() -> bool {
    env::var("LISETTE_NO_CACHE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;
    use syntax::ast::{Annotation, Generic, Span, StructFields};
    use syntax::program::{Attributes, ConstantValue, Definition, DefinitionBody, Visibility};
    use syntax::types::{FunctionParameter, Symbol, Type};

    fn load_test_cache(
        package_id: &str,
        full_hash: u64,
        dep_hashes: &FxHashMap<String, u64>,
        artifact_hash: Option<u64>,
        root: &Path,
        target: Target,
    ) -> Option<PackageInterface> {
        try_load_cache(
            &CompiledPackage {
                package_id: package_id.into(),
                key: CacheKey::new(CacheKeyInputs {
                    target,
                    sources: full_hash,
                    dependencies: dep_hashes.clone(),
                    third_party_typedefs: String::new(),
                    diagnostic_mode: DiagnosticMode::default(),
                }),
                artifact_hash: artifact_hash.unwrap_or_default(),
                go_method_context: 0,
            },
            root,
            target,
        )
    }

    fn generic_struct_definition(visibility: Visibility, file_id: u32) -> Definition {
        let bound_span = Span::new(file_id, 12, 5);
        Definition {
            visibility,
            ty: Type::Nominal {
                id: Symbol::from_raw("package.Box"),
                params: vec![Type::Parameter("T".into())],
                writable: false,
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
                            writable: false,
                            mut_span: None,
                            span: bound_span,
                        },
                        Type::Nominal {
                            id: Symbol::from_raw("prelude.Comparable"),
                            params: Vec::new(),
                            writable: false,
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

        let deps = FxHashMap::default();
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
        let mut deps1 = FxHashMap::default();
        deps1.insert("dep_a".to_string(), 111u64);

        let mut deps2 = FxHashMap::default();
        deps2.insert("dep_a".to_string(), 222u64);

        let hash1 = compute_package_hash(source_hash, &deps1);
        let hash2 = compute_package_hash(source_hash, &deps2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_package_hash_deterministic() {
        let source_hash = 12345u64;
        let mut deps = FxHashMap::default();
        deps.insert("dep_b".to_string(), 222u64);
        deps.insert("dep_a".to_string(), 111u64);

        let hash1 = compute_package_hash(source_hash, &deps);
        let hash2 = compute_package_hash(source_hash, &deps);

        assert_eq!(hash1, hash2);
    }

    fn test_key(sources: u64, dependencies: FxHashMap<String, u64>) -> CacheKey {
        CacheKey::new(CacheKeyInputs {
            target: Target::host(),
            sources,
            dependencies,
            third_party_typedefs: String::new(),
            diagnostic_mode: DiagnosticMode::default(),
        })
    }

    #[test]
    fn cache_key_checks_format_version() {
        let current = test_key(100, FxHashMap::default());
        let older = CacheKey {
            format_version: CACHE_FORMAT_VERSION + 1,
            ..current.clone()
        };
        assert_ne!(older, current);
    }

    #[test]
    fn cache_key_checks_compiler_build() {
        let current = test_key(100, FxHashMap::default());
        let other_build = CacheKey {
            compiler_build: COMPILER_BUILD_HASH + 1,
            ..current.clone()
        };
        assert_ne!(other_build, current);
    }

    #[test]
    fn compiler_build_hash_differs_from_version_alone() {
        let mut hasher = FnvHasher::new();
        hasher.write(env!("CARGO_PKG_VERSION").as_bytes());
        assert_ne!(COMPILER_BUILD_HASH, hasher.finish());
    }

    #[test]
    fn cache_key_checks_sources() {
        assert_ne!(
            test_key(200, FxHashMap::default()),
            test_key(100, FxHashMap::default())
        );
        assert_eq!(
            test_key(100, FxHashMap::default()),
            test_key(100, FxHashMap::default())
        );
    }

    #[test]
    fn cache_key_checks_target() {
        let host = test_key(100, FxHashMap::default());
        let cross = CacheKey::new(CacheKeyInputs {
            target: Target::new("plan9", "386"),
            sources: 100,
            dependencies: FxHashMap::default(),
            third_party_typedefs: String::new(),
            diagnostic_mode: DiagnosticMode::default(),
        });
        assert_ne!(host, cross);
    }

    #[test]
    fn cache_key_checks_third_party_typedefs() {
        let before = test_key(100, FxHashMap::default());
        let after = CacheKey {
            third_party_typedefs: "other".into(),
            ..before.clone()
        };
        assert_ne!(before, after);
    }

    #[test]
    fn go_dependencies_use_the_stdlib_hash() {
        let hashes = get_dependency_package_hashes(
            ["go:example.com/lib", "go:fmt", "prelude"].iter().copied(),
            &FxHashMap::default(),
        );
        assert!(hashes.values().all(|hash| *hash == STDLIB_HASH));
    }

    #[test]
    fn try_load_cache_keeps_the_file_on_a_mode_only_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let target = Target::host();
        let mut interface = PackageInterface {
            key: test_key(100, FxHashMap::default()),
            diagnostics: Vec::new(),
            files: vec![],
            definitions: FxHashMap::default(),
            emit_stamp: None,
            go_method_context: 0,
        };
        interface.key.diagnostic_mode =
            DiagnosticMode::new(CompilePhase::Check, ProjectKind::Library);
        let path = cache_path(root, "greet", target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        assert!(load_test_cache("greet", 100, &FxHashMap::default(), None, root, target).is_none());
        assert!(path.exists(), "a mode-only mismatch must keep the file");
        assert!(load_test_cache("greet", 200, &FxHashMap::default(), None, root, target).is_none());
        assert!(!path.exists(), "any other mismatch removes the file");
    }

    #[test]
    fn mode_only_difference_keeps_file() {
        let check = test_key(100, FxHashMap::default());
        let emit = CacheKey {
            diagnostic_mode: DiagnosticMode::new(CompilePhase::Check, ProjectKind::Library),
            ..check.clone()
        };
        assert!(check.differs_only_in_mode(&emit));
        let other_sources = CacheKey {
            sources: 1,
            ..emit.clone()
        };
        assert!(!check.differs_only_in_mode(&other_sources));
        assert!(!check.differs_only_in_mode(&check));
    }

    #[test]
    fn build_cached_package_preserves_constant_kind() {
        use syntax::program::{Definition, DefinitionBody, ValueKind, Visibility};

        let make_value = |kind| Definition {
            visibility: Visibility::Public,
            ty: Type::Nominal {
                id: Symbol::from_raw("int"),
                params: vec![],
                writable: false,
            },
            name_span: None,
            doc: None,
            body: DefinitionBody::Value {
                kind,
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                go_type_param_recipe: None,
                go_type_terms: Vec::new(),
                superseded_by: None,
                impl_receiver: None,
            },
        };

        let empty_files = FxHashMap::default();
        let const_def = make_value(ValueKind::Constant(ConstantValue::Integer {
            value: 5,
            text: None,
        }));
        let declaration_def = make_value(ValueKind::ConstantDeclaration);
        let var_def = make_value(ValueKind::Variable);

        let mut definitions = FxHashMap::default();
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
            key: test_key(0, FxHashMap::default()),
            diagnostics: Vec::new(),
            files: vec![],
            definitions,
            emit_stamp: None,
            go_method_context: 0,
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
                writable: false,
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

        let empty_files = FxHashMap::default();
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
                    writable: false,
                }),
            )
        );
    }

    #[test]
    fn test_cache_validity_checks_dep_hashes() {
        let mut cached_deps = FxHashMap::default();
        cached_deps.insert("dep".to_string(), 111u64);

        let cached = test_key(100, cached_deps.clone());

        let mut different_deps = FxHashMap::default();
        different_deps.insert("dep".to_string(), 222u64);

        assert_ne!(cached, test_key(100, different_deps));
        assert_eq!(cached, test_key(100, cached_deps));
    }

    #[test]
    fn test_type_roundtrip_bincode() {
        let ty = Type::function(
            vec![FunctionParameter::new(Type::Nominal {
                id: Symbol::from_raw("int"),
                params: vec![],
                writable: false,
            })],
            vec![],
            Box::new(Type::Nominal {
                id: Symbol::from_raw("main.MyType"),
                params: vec![Type::Tuple(vec![Type::Never])],
                writable: false,
            }),
        );

        let bytes = bincode::serialize(&ty).unwrap();
        let restored: Type = bincode::deserialize(&bytes).unwrap();
        assert_eq!(ty, restored);
    }

    #[test]
    fn test_cache_path_format() {
        let target = Target::new("linux", "amd64");
        let path = cache_path(Path::new("/project"), "utils", target);
        assert_eq!(
            path,
            PathBuf::from("/project/target/.lisette/cache/linux_amd64/utils.cache")
        );

        let path = cache_path(Path::new("/project"), "deep/nested/mod", target);
        assert_eq!(
            path,
            PathBuf::from("/project/target/.lisette/cache/linux_amd64/deep_snested_smod.cache")
        );
    }

    #[test]
    fn cache_path_separates_targets() {
        assert_ne!(
            cache_path(
                Path::new("/project"),
                "utils",
                Target::new("linux", "amd64")
            ),
            cache_path(
                Path::new("/project"),
                "utils",
                Target::new("linux", "arm64")
            ),
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
        let mut edges = FxHashMap::default();
        let mut deps = FxHashSet::default();
        deps.insert("go:fmt".to_string());
        deps.insert("prelude".to_string());
        deps.insert("user_mod".to_string());
        edges.insert("my_mod".to_string(), deps);

        let mut package_hashes = FxHashMap::default();
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
        let h1 = compute_emit_artifact_hash(100, "github.com/old/proj", 0);
        let h2 = compute_emit_artifact_hash(100, "github.com/new/proj", 0);
        assert_ne!(h1, h2);
    }

    #[test]
    fn apply_emit_stamps_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let target = Target::host();

        let interface = PackageInterface {
            key: test_key(100, FxHashMap::default()),
            diagnostics: Vec::new(),
            files: vec![],
            definitions: FxHashMap::default(),
            emit_stamp: None,
            go_method_context: 0,
        };
        let path = cache_path(root, "greet", target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        let stamp = EmitStamp {
            package_id: "greet".to_string(),
            artifact_hash: 999,
        };
        apply_emit_stamps(root, &[(stamp.clone(), Some(999))], target).unwrap();
        let reread: PackageInterface = bincode::deserialize(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.emit_stamp, Some(999));
        assert_eq!(reread.key, test_key(100, FxHashMap::default()));

        apply_emit_stamps(root, &[(stamp, None)], target).unwrap();
        let reread: PackageInterface = bincode::deserialize(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread.emit_stamp, None);
    }

    #[test]
    fn apply_emit_stamps_missing_cache_is_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let stamp = EmitStamp {
            package_id: "absent".to_string(),
            artifact_hash: 0,
        };
        let result = apply_emit_stamps(tmp.path(), &[(stamp, None)], Target::host());
        assert!(result.is_ok());
    }

    #[test]
    fn apply_emit_stamps_removes_corrupt_cache() {
        let temp = tempfile::tempdir().unwrap();
        let path = cache_path(temp.path(), "corrupt", Target::host());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"invalid").unwrap();
        let stamp = EmitStamp {
            package_id: "corrupt".to_string(),
            artifact_hash: 0,
        };

        let result = apply_emit_stamps(temp.path(), &[(stamp, None)], Target::host());

        assert_eq!((result.is_ok(), path.exists()), (true, false));
    }

    #[test]
    fn clear_emit_stamps_touches_one_target_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let host = Target::new("darwin", "arm64");
        let cross = Target::new("linux", "amd64");

        let stamped = |target| {
            let interface = PackageInterface {
                key: test_key(100, FxHashMap::default()),
                diagnostics: Vec::new(),
                files: vec![],
                definitions: FxHashMap::default(),
                emit_stamp: Some(7),
                go_method_context: 0,
            };
            let path = cache_path(root, "greet", target);
            disk::write(&path, &interface).unwrap();
            path
        };
        let host_path = stamped(host);
        let cross_path = stamped(cross);

        clear_emit_stamps(root, host).unwrap();

        let reread = |path| disk::read::<PackageInterface>(path).unwrap();
        assert_eq!(reread(&host_path).emit_stamp, None);
        assert_eq!(reread(&cross_path).emit_stamp, Some(7));
    }

    #[test]
    fn clear_emit_stamps_without_a_cache_dir_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(clear_emit_stamps(tmp.path(), Target::host()).is_ok());
    }

    #[test]
    fn semantic_cache_accepts_unstamped_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let target = Target::host();
        fs::create_dir_all(root.join("target").join("greet")).unwrap();
        fs::write(root.join("target").join("greet").join("greet.go"), "").unwrap();

        let interface = PackageInterface {
            key: test_key(100, FxHashMap::default()),
            diagnostics: Vec::new(),
            files: vec![CachedFile {
                name: "greet.lis".to_string(),
                source: String::new(),
            }],
            definitions: FxHashMap::default(),
            emit_stamp: None,
            go_method_context: 0,
        };
        let path = cache_path(root, "greet", target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        let loaded = load_test_cache("greet", 100, &FxHashMap::default(), None, root, target);
        assert!(loaded.is_some(), "Check phase must accept unstamped cache");

        let loaded = load_test_cache(
            "greet",
            100,
            &FxHashMap::default(),
            Some(compute_emit_artifact_hash(100, "github.com/test/x", 0)),
            root,
            target,
        );
        assert!(
            loaded.is_some(),
            "Missing Go artifacts must not discard semantic interfaces"
        );
    }

    #[test]
    fn semantic_cache_keeps_interface_after_sourcemap_invalidation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let target = Target::host();
        fs::create_dir_all(root.join("target").join("greet")).unwrap();
        fs::write(root.join("target").join("greet").join("greet.go"), "").unwrap();

        let artifact_hash = compute_emit_artifact_hash(100, "github.com/test/x", 0);

        let interface = PackageInterface {
            key: test_key(100, FxHashMap::default()),
            diagnostics: Vec::new(),
            files: vec![CachedFile {
                name: "greet.lis".to_string(),
                source: String::new(),
            }],
            definitions: FxHashMap::default(),
            emit_stamp: Some(artifact_hash),
            go_method_context: 0,
        };
        let path = cache_path(root, "greet", target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bincode::serialize(&interface).unwrap()).unwrap();

        assert!(
            load_test_cache(
                "greet",
                100,
                &FxHashMap::default(),
                Some(artifact_hash),
                root,
                target,
            )
            .is_some()
        );

        let stamp = EmitStamp {
            package_id: "greet".to_string(),
            artifact_hash,
        };
        apply_emit_stamps(root, &[(stamp, None)], target).unwrap();

        assert!(
            load_test_cache(
                "greet",
                100,
                &FxHashMap::default(),
                Some(artifact_hash),
                root,
                target,
            )
            .is_some()
        );
    }

    #[test]
    fn diagnostic_cache_remaps_all_labels_and_fix_edits() {
        use diagnostics::{Edit, Fix};

        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new();
        store.store_package(
            "example",
            vec![
                File::new_cached("example", "z.lis", "z.lis", "héllo", 91),
                File::new_cached("example", "a.lis", "a.lis", "second", 37),
            ],
        );
        let diagnostic = diagnostics::lint::unused_mut(&Span::new(91, 1, 2))
            .with_span_primary_label(&Span::new(37, 0, 2), "related declaration")
            .with_fix(Fix::multi(
                "fix both places",
                Edit::deletion(Span::new(91, 0, 1)),
                vec![Edit::replacement(Span::new(91, 3, 3), "new")],
            ));
        let compiled = CompiledPackage {
            package_id: "example".into(),
            artifact_hash: 1,
            key: test_key(2, FxHashMap::default()),
            go_method_context: 0,
        };
        save_package_cache(
            &compiled,
            &store,
            root.path(),
            Target::host(),
            &[&diagnostic],
        )
        .unwrap();
        let mut cache = load_test_cache(
            "example",
            2,
            &FxHashMap::default(),
            None,
            root.path(),
            Target::host(),
        )
        .unwrap();
        let replayed = cache.take_diagnostics(200);
        let expected = diagnostic
            .try_map_spans(|span| {
                Some(Span {
                    file_id: if span.file_id == 37 { 200 } else { 201 },
                    ..span
                })
            })
            .unwrap();
        assert_eq!(
            bincode::serialize(&replayed).unwrap(),
            bincode::serialize(&vec![expected]).unwrap()
        );
    }

    #[test]
    fn diagnostic_cache_rejects_foreign_files() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new();
        store.store_package(
            "example",
            vec![File::new_cached("example", "a.lis", "a.lis", "hello", 37)],
        );
        let compiled = CompiledPackage {
            package_id: "example".into(),
            artifact_hash: 1,
            key: test_key(2, FxHashMap::default()),
            go_method_context: 0,
        };
        let foreign = diagnostics::lint::unused_mut(&Span::new(37, 0, 1))
            .with_span_label(&Span::new(90, 0, 1), "outside package");
        save_package_cache(&compiled, &store, root.path(), Target::host(), &[&foreign]).unwrap();
        assert!(!cache_path(root.path(), "example", Target::host()).exists());
    }

    #[test]
    fn go_outputs_found_for_file_with_lis_inside_its_name() {
        let root = tempfile::tempdir().unwrap();
        let target_dir = root.path().join("target").join("sub");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("util.lister.go"), "").unwrap();
        let cached_files = [CachedFile {
            name: "util.lister.lis".into(),
            source: String::new(),
        }];
        assert!(all_go_outputs_exist("sub", &cached_files, root.path()));
    }
}
