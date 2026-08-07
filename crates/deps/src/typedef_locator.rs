use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use stdlib::Target;

use crate::project_manifest::{
    GoDependency, Manifest, ReplacementSource, check_no_subpackage_deps, check_toolchain_version,
    find_module_for_pkg, parse_manifest,
};
use crate::{GoModule, GoPackage, ReplacementTarget, typedef_cache_dir};

/// `(module path, version to hand Go, optional replacement)`.
type ResolvedModule = (String, String, Option<ResolvedReplacement>);

#[derive(Debug, Clone)]
pub enum ResolvedReplacement {
    Module { path: String, version: String },
    Local,
}

impl ResolvedReplacement {
    pub fn as_target(&self) -> ReplacementTarget<'_> {
        match self {
            ResolvedReplacement::Module { path, version } => {
                ReplacementTarget::Module(crate::Replacement { path, version })
            }
            ResolvedReplacement::Local => ReplacementTarget::LocalDirectory,
        }
    }
}

#[derive(Debug)]
pub enum TypedefLocatorResult {
    Found {
        content: Cow<'static, str>,
        origin: TypedefOrigin,
    },
    /// Looks like a stdlib package but no stdlib typedef exists.
    UnknownStdlib,
    /// Has a domain-style path but is not declared in the manifest.
    UndeclaredImport,
    /// An `internal/` package of a dependency, unimportable across modules.
    InternalPackage { module: String },
    /// Declared in the manifest but no `.d.lis` on disk and no bindgen runner.
    MissingTypedef {
        module: String,
        version: String,
        replacement_path: Option<String>,
        local: bool,
    },
    /// Typedef file exists but could not be read.
    UnreadableTypedef { path: PathBuf, error: String },
    /// The bindgen runner ran on cache miss but failed.
    BindgenFailed {
        module: String,
        version: String,
        package: String,
        kind: BindgenFailure,
    },
}

/// Why a `Bindgen::run` invocation failed.
#[derive(Debug)]
pub enum BindgenFailure {
    /// `go` is not installed or not on PATH.
    GoToolchainMissing,
    /// The bindgen subprocess failed; `stderr` is the trimmed message.
    InvocationFailed { stderr: String },
}

/// Classification of a `go:` import path without touching the cache.
#[derive(Debug)]
pub enum DeclarationStatus {
    Stdlib,
    DeclaredThirdParty {
        module: String,
        version: String,
    },
    DeclaredReplacement {
        module: String,
        replacement_path: String,
        replacement_version: String,
    },
    DeclaredLocal {
        module: String,
        path: String,
    },
    /// An `internal/` package of a dependency, which Go forbids importing
    /// across module boundaries.
    InternalPackage {
        module: String,
    },
    UnknownStdlib,
    UndeclaredImport,
}

#[derive(Debug)]
pub struct ImportablePackage {
    /// Import path without the `go:` prefix, e.g. `net/http`.
    pub path: String,
    typedef: PackageTypedef,
}

#[derive(Debug)]
enum PackageTypedef {
    Embedded(&'static str),
    Cached(PathBuf),
}

impl ImportablePackage {
    pub fn typedef_source(&self) -> Option<Cow<'static, str>> {
        match &self.typedef {
            PackageTypedef::Embedded(source) => Some(Cow::Borrowed(source)),
            PackageTypedef::Cached(path) => std::fs::read_to_string(path).ok().map(Cow::Owned),
        }
    }

    pub fn declared_package_name(&self) -> Option<String> {
        match &self.typedef {
            PackageTypedef::Embedded(source) => {
                stdlib::declared_package_name(source).map(str::to_string)
            }
            PackageTypedef::Cached(path) => {
                let mut reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
                let mut header = String::new();
                for _ in 0..stdlib::HEADER_LINES {
                    if std::io::BufRead::read_line(&mut reader, &mut header).ok()? == 0 {
                        break;
                    }
                }
                stdlib::declared_package_name(&header).map(str::to_string)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedefOrigin {
    Stdlib,
    Cache(PathBuf),
}

impl TypedefOrigin {
    /// Consume the origin, yielding the on-disk path for cache origins and
    /// `None` for embedded stdlib typedefs.
    pub fn into_cache_path(self) -> Option<PathBuf> {
        match self {
            TypedefOrigin::Cache(path) => Some(path),
            TypedefOrigin::Stdlib => None,
        }
    }
}

/// Cache-miss hook the locator invokes for declared third-party packages.
pub trait Bindgen: Send + Sync + std::fmt::Debug {
    /// Generate `pkg`'s typedef and write it to the cache (no transitives).
    fn run(&self, pkg: &GoPackage) -> Result<(), BindgenFailure>;
}

/// Per-analysis bindgen runner; dropping the session releases the lock.
pub struct BindgenSession {
    pub bindgen: std::sync::Arc<dyn Bindgen>,
    _guard: Box<dyn BindgenGuard>,
}

impl BindgenSession {
    pub fn new(bindgen: std::sync::Arc<dyn Bindgen>, guard: Box<dyn BindgenGuard>) -> Self {
        Self {
            bindgen,
            _guard: guard,
        }
    }
}

pub trait BindgenGuard: sealed::Sealed + Send {}

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for std::fs::File {}
impl BindgenGuard for std::fs::File {}

pub trait BindgenSetup: Send + Sync {
    fn for_project(&self, project_root: &Path, target: Target) -> Result<BindgenSession, String>;
}

#[derive(Debug, Clone, Default)]
pub struct TypedefLocator {
    deps: BTreeMap<String, GoDependency>,
    project_root: Option<PathBuf>,
    target: Target,
    bindgen: Option<Arc<dyn Bindgen>>,
    /// Computed at most once per locator: one build sees one snapshot of local source.
    local_stamp: Arc<OnceLock<Option<String>>>,
}

impl TypedefLocator {
    pub fn new(
        deps: BTreeMap<String, GoDependency>,
        project_root: Option<PathBuf>,
        target: Target,
    ) -> Self {
        Self {
            deps,
            project_root,
            target,
            bindgen: None,
            local_stamp: Arc::new(OnceLock::new()),
        }
    }

    pub fn declared_stamp(&self) -> String {
        crate::local_source::stamp_hash(&self.deps, None)
    }

    pub fn with_fresh_local_stamp(&self) -> Self {
        Self {
            local_stamp: Arc::new(OnceLock::new()),
            ..self.clone()
        }
    }

    /// The stamp value gating local typedefs, `None` when none are declared.
    pub fn local_stamp_value(&self) -> Option<&str> {
        self.local_stamp
            .get_or_init(|| {
                let project_root = self.project_root.as_deref()?;
                let has_local = self.deps.values().any(|dep| {
                    matches!(
                        dep,
                        GoDependency::Replaced {
                            source: ReplacementSource::Local { .. },
                            ..
                        }
                    )
                });
                has_local.then(|| crate::local_source::stamp_hash(&self.deps, Some(project_root)))
            })
            .as_deref()
    }

    pub fn from_project(project_root: &Path) -> Result<Self, String> {
        let (_, locator) = Self::from_project_with_manifest(project_root)?;
        Ok(locator)
    }

    pub fn from_project_with_manifest(project_root: &Path) -> Result<(Manifest, Self), String> {
        let manifest = parse_manifest(project_root)?;

        check_toolchain_version(&manifest)?;
        check_no_subpackage_deps(&manifest)?;

        let locator = Self::new(
            manifest.go_deps(),
            Some(project_root.to_path_buf()),
            Target::host(),
        );

        Ok((manifest, locator))
    }

    pub fn with_bindgen(mut self, bindgen: Arc<dyn Bindgen>) -> Self {
        self.bindgen = Some(bindgen);
        self
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    pub fn deps(&self) -> &BTreeMap<String, GoDependency> {
        &self.deps
    }

    pub fn target(&self) -> Target {
        self.target
    }

    pub fn is_declared_go_dep(&self, package_path: &str) -> bool {
        find_module_for_pkg(&self.deps, package_path).is_some()
    }

    /// Resolve a `go:` package path to its declared module, the version to hand
    /// Go, and its replacement if any, by longest declared prefix. `None` if no
    /// declared module contains it.
    pub fn module_for_package(&self, package_path: &str) -> Option<ResolvedModule> {
        let (module, dep) = find_module_for_pkg(&self.deps, package_path)?;
        let module = module.to_string();
        Some(match dep {
            GoDependency::Remote { version, .. } => (module, version.clone(), None),
            GoDependency::Replaced { source, .. } => {
                let synthetic = crate::placeholder_require_version(&module);
                let replacement = match source {
                    ReplacementSource::Module { path, version } => ResolvedReplacement::Module {
                        path: path.clone(),
                        version: version.clone(),
                    },
                    ReplacementSource::Local { .. } => ResolvedReplacement::Local,
                };
                (module, synthetic, Some(replacement))
            }
        })
    }

    /// Classify a `go:` import path without touching the cache or bindgen.
    pub fn validate_declaration(&self, package_path: &str) -> DeclarationStatus {
        self.classify(package_path)
    }

    fn classify(&self, package_path: &str) -> DeclarationStatus {
        if crate::is_stdlib(package_path) {
            return match stdlib::get_go_stdlib_typedef(package_path, self.target) {
                Some(_) => DeclarationStatus::Stdlib,
                None => DeclarationStatus::UnknownStdlib,
            };
        }

        if let Some((module_path, _)) = find_module_for_pkg(&self.deps, package_path)
            && package_path
                .strip_prefix(module_path)
                .is_some_and(|rest| rest.split('/').any(|segment| segment == "internal"))
        {
            return DeclarationStatus::InternalPackage {
                module: module_path.to_string(),
            };
        }
        match find_module_for_pkg(&self.deps, package_path) {
            Some((module_path, GoDependency::Remote { version, .. })) => {
                DeclarationStatus::DeclaredThirdParty {
                    module: module_path.to_string(),
                    version: version.clone(),
                }
            }
            Some((
                module_path,
                GoDependency::Replaced {
                    source: ReplacementSource::Module { path, version },
                    ..
                },
            )) => DeclarationStatus::DeclaredReplacement {
                module: module_path.to_string(),
                replacement_path: path.clone(),
                replacement_version: version.clone(),
            },
            Some((
                module_path,
                GoDependency::Replaced {
                    source: ReplacementSource::Local { path },
                    ..
                },
            )) => DeclarationStatus::DeclaredLocal {
                module: module_path.to_string(),
                path: path.clone(),
            },
            None => DeclarationStatus::UndeclaredImport,
        }
    }

    pub fn importable_packages(&self) -> Vec<ImportablePackage> {
        let mut packages: Vec<ImportablePackage> = stdlib::get_go_stdlib_packages(self.target)
            .into_iter()
            .filter_map(|path| {
                Some(ImportablePackage {
                    path: path.to_string(),
                    typedef: PackageTypedef::Embedded(stdlib::get_go_stdlib_typedef(
                        path,
                        self.target,
                    )?),
                })
            })
            .collect();

        let Some(project_root) = &self.project_root else {
            return packages;
        };
        let cache_dir = typedef_cache_dir(project_root);

        for module_path in self.deps.keys() {
            let Some((module, version, replacement)) = self.module_for_package(module_path) else {
                continue;
            };
            let pkg = GoPackage {
                module: GoModule {
                    path: &module,
                    version: &version,
                    replacement: replacement.as_ref().map(ResolvedReplacement::as_target),
                },
                package: &module,
            };
            let local_stamp = match replacement {
                Some(ResolvedReplacement::Local) => self.local_stamp_value(),
                _ => None,
            };

            let root_typedef = pkg.typedef_path(&cache_dir, self.target);
            if let Some(module_dir) = root_typedef.parent() {
                collect_cached_packages(module_dir, &module, local_stamp, &mut packages);
            }
        }

        packages
    }

    /// Resolve a `go:` package: stdlib -> on-disk cache -> bindgen runner if set.
    pub fn find_typedef_content(&self, package_path: &str) -> TypedefLocatorResult {
        let (module_path, version, replacement) = match self.classify(package_path) {
            DeclarationStatus::Stdlib => {
                let source = stdlib::get_go_stdlib_typedef(package_path, self.target)
                    .expect("Stdlib classification implies an embedded typedef");
                // Record the path the editor will navigate to; the files are
                // written by the LSP at startup. No HOME -> non-navigable.
                let origin = match crate::stdlib_typedef_path(self.target, package_path) {
                    Some(path) => TypedefOrigin::Cache(path),
                    None => TypedefOrigin::Stdlib,
                };
                return TypedefLocatorResult::Found {
                    content: Cow::Borrowed(source),
                    origin,
                };
            }
            DeclarationStatus::UnknownStdlib => return TypedefLocatorResult::UnknownStdlib,
            DeclarationStatus::UndeclaredImport => return TypedefLocatorResult::UndeclaredImport,
            DeclarationStatus::InternalPackage { module } => {
                return TypedefLocatorResult::InternalPackage { module };
            }
            DeclarationStatus::DeclaredThirdParty { module, version } => (module, version, None),
            DeclarationStatus::DeclaredReplacement {
                module,
                replacement_path,
                replacement_version,
            } => {
                let synthetic = crate::placeholder_require_version(&module);
                (
                    module,
                    synthetic,
                    Some(ResolvedReplacement::Module {
                        path: replacement_path,
                        version: replacement_version,
                    }),
                )
            }
            DeclarationStatus::DeclaredLocal { module, .. } => {
                let synthetic = crate::placeholder_require_version(&module);
                (module, synthetic, Some(ResolvedReplacement::Local))
            }
        };

        let is_local = matches!(replacement, Some(ResolvedReplacement::Local));
        let display_version = match &replacement {
            Some(ResolvedReplacement::Module { version, .. }) => version.clone(),
            Some(ResolvedReplacement::Local) | None => version.clone(),
        };
        let replacement_path = match &replacement {
            Some(ResolvedReplacement::Module { path, .. }) => Some(path.clone()),
            Some(ResolvedReplacement::Local) | None => None,
        };

        let Some(project_root) = &self.project_root else {
            return TypedefLocatorResult::MissingTypedef {
                module: module_path,
                version: display_version,
                replacement_path,
                local: is_local,
            };
        };

        let pkg = GoPackage {
            module: GoModule {
                path: &module_path,
                version: &version,
                replacement: replacement.as_ref().map(ResolvedReplacement::as_target),
            },
            package: package_path,
        };
        let cache_dir = typedef_cache_dir(project_root);
        let typedef_path = pkg.typedef_path(&cache_dir, self.target);

        if is_local
            && let Some(stamp) = self.local_stamp_value()
            && let Err(error) = crate::local_source::gate_local_typedef(&typedef_path, stamp)
        {
            return TypedefLocatorResult::UnreadableTypedef {
                path: typedef_path,
                error: format!(
                    "the local module changed but its stale typedef could not be removed: {}",
                    error
                ),
            };
        }

        match read_cached_typedef(&typedef_path) {
            Some(found) => found,
            None => match &self.bindgen {
                None => TypedefLocatorResult::MissingTypedef {
                    module: module_path,
                    version: display_version,
                    replacement_path,
                    local: is_local,
                },
                Some(runner) => match runner.run(&pkg) {
                    Ok(()) => read_cached_typedef(&typedef_path).unwrap_or_else(|| {
                        TypedefLocatorResult::BindgenFailed {
                            module: module_path,
                            version: display_version,
                            package: package_path.to_string(),
                            kind: BindgenFailure::InvocationFailed {
                                stderr: format!(
                                    "bindgen reported success but `{}` was not written",
                                    typedef_path.display()
                                ),
                            },
                        }
                    }),
                    Err(kind) => TypedefLocatorResult::BindgenFailed {
                        module: module_path,
                        version: display_version,
                        package: package_path.to_string(),
                        kind,
                    },
                },
            },
        }
    }
}

fn collect_cached_packages(
    dir: &Path,
    package_path: &str,
    local_stamp: Option<&str>,
    out: &mut Vec<ImportablePackage>,
) {
    let last_segment = package_path.rsplit('/').next().unwrap_or(package_path);
    let typedef = dir.join(format!("{last_segment}.d.lis"));
    let fresh = local_stamp
        .is_none_or(|stamp| crate::local_source::local_typedef_is_fresh(&typedef, stamp));
    if fresh && typedef.is_file() {
        out.push(ImportablePackage {
            path: package_path.to_string(),
            typedef: PackageTypedef::Cached(typedef),
        });
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == "internal" {
            continue;
        }
        collect_cached_packages(
            &entry.path(),
            &format!("{package_path}/{name}"),
            local_stamp,
            out,
        );
    }
}

enum ReadOutcome {
    Found(String),
    Missing,
    Unreadable(String),
}

fn read_typedef(path: &Path) -> ReadOutcome {
    match std::fs::read_to_string(path) {
        Ok(s) => ReadOutcome::Found(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ReadOutcome::Missing,
        Err(e) => ReadOutcome::Unreadable(e.to_string()),
    }
}

/// Reads a cached typedef file, wrapping a hit or a read error; `None` on a cache miss.
fn read_cached_typedef(path: &Path) -> Option<TypedefLocatorResult> {
    match read_typedef(path) {
        ReadOutcome::Found(content) => Some(TypedefLocatorResult::Found {
            content: Cow::Owned(content),
            origin: TypedefOrigin::Cache(path.to_path_buf()),
        }),
        ReadOutcome::Unreadable(error) => Some(TypedefLocatorResult::UnreadableTypedef {
            path: path.to_path_buf(),
            error,
        }),
        ReadOutcome::Missing => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn write_typedef(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn importable_packages_walk_a_cached_module_tree() {
        let project = tempfile::tempdir().unwrap();
        let mut deps = BTreeMap::new();
        deps.insert(
            "github.com/example/go-lib".to_string(),
            GoDependency::Remote {
                version: "v1.2.0".to_string(),
                via: None,
            },
        );
        let locator = TypedefLocator::new(deps, Some(project.path().to_path_buf()), Target::host());

        let module_dir = typedef_cache_dir(project.path())
            .join(Target::host().cache_segment())
            .join("github.com/example/go-lib@v1.2.0");
        write_typedef(&module_dir.join("go-lib.d.lis"), "// Package: lib\n");
        write_typedef(&module_dir.join("mux/mux.d.lis"), "pub fn New() -> int\n");
        write_typedef(&module_dir.join("internal/hide/hide.d.lis"), "pub fn X()\n");

        let mut found: Vec<(String, Option<String>)> = locator
            .importable_packages()
            .into_iter()
            .filter(|package| crate::is_third_party(&package.path))
            .map(|package| (package.path.clone(), package.declared_package_name()))
            .collect();
        found.sort();

        assert_eq!(
            found,
            vec![
                (
                    "github.com/example/go-lib".to_string(),
                    Some("lib".to_string())
                ),
                ("github.com/example/go-lib/mux".to_string(), None),
            ],
            "subpackages come along, another module's internal packages do not"
        );
    }

    #[test]
    fn importable_packages_include_the_stdlib_with_its_embedded_typedef() {
        let locator = TypedefLocator::new(BTreeMap::new(), None, Target::host());

        let strings = locator
            .importable_packages()
            .into_iter()
            .find(|package| package.path == "strings")
            .expect("the stdlib is importable without a project");

        assert!(
            strings
                .typedef_source()
                .is_some_and(|source| source.contains("pub fn ToLower")),
            "the embedded typedef comes with the package"
        );
    }

    #[test]
    fn replace_diagnostic_shows_replacement_version_not_synthetic() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "github.com/df-mc/dragonfly".to_string(),
            GoDependency::Replaced {
                source: ReplacementSource::Module {
                    path: "github.com/you/dragonfly".to_string(),
                    version: "v1.9.0".to_string(),
                },
                via: None,
            },
        );
        let locator = TypedefLocator::new(deps, None, Target::host());

        match locator.find_typedef_content("github.com/df-mc/dragonfly") {
            TypedefLocatorResult::MissingTypedef {
                module,
                version,
                replacement_path,
                local,
            } => {
                assert_eq!(module, "github.com/df-mc/dragonfly");
                assert_eq!(version, "v1.9.0");
                assert_eq!(
                    replacement_path.as_deref(),
                    Some("github.com/you/dragonfly")
                );
                assert!(!local);
            }
            other => panic!("expected MissingTypedef, got {:?}", other),
        }
    }

    #[test]
    fn internal_packages_of_dependencies_are_rejected_at_classification() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "github.com/gorilla/mux".to_string(),
            GoDependency::Remote {
                version: "v1.8.0".to_string(),
                via: None,
            },
        );
        deps.insert(
            "example.com/me/foo".to_string(),
            GoDependency::Replaced {
                source: ReplacementSource::Local {
                    path: "../foo".to_string(),
                },
                via: None,
            },
        );
        let locator = TypedefLocator::new(deps, None, Target::host());

        for pkg in [
            "github.com/gorilla/mux/internal/x",
            "example.com/me/foo/internal",
        ] {
            assert!(
                matches!(
                    locator.validate_declaration(pkg),
                    DeclarationStatus::InternalPackage { .. }
                ),
                "{}",
                pkg
            );
        }
        assert!(!matches!(
            locator.validate_declaration("example.com/me/foo/internally"),
            DeclarationStatus::InternalPackage { .. }
        ));
    }

    fn local_project(module: &str, path: &str) -> (tempfile::TempDir, TypedefLocator) {
        let project = tempfile::tempdir().unwrap();
        let module_dir = project.path().join(path);
        std::fs::create_dir_all(&module_dir).unwrap();
        std::fs::write(
            module_dir.join("go.mod"),
            format!("module {}\n\ngo 1.25\n", module),
        )
        .unwrap();
        std::fs::write(module_dir.join("lib.go"), "package lib\n").unwrap();

        let mut deps = BTreeMap::new();
        deps.insert(
            module.to_string(),
            GoDependency::Replaced {
                source: ReplacementSource::Local {
                    path: path.to_string(),
                },
                via: None,
            },
        );
        let locator = TypedefLocator::new(deps, Some(project.path().to_path_buf()), Target::host());
        (project, locator)
    }

    #[test]
    fn local_stamp_gate_discards_typedef_when_local_source_changes() {
        let module = "example.com/me/foo";
        let (project, locator) = local_project(module, "foo");

        let pkg = GoPackage {
            module: GoModule {
                path: module,
                version: "v0.0.0",
                replacement: Some(crate::ReplacementTarget::LocalDirectory),
            },
            package: module,
        };
        let typedef_path = pkg.typedef_path(&typedef_cache_dir(project.path()), Target::host());
        std::fs::create_dir_all(typedef_path.parent().unwrap()).unwrap();
        std::fs::write(&typedef_path, "// Generated\n").unwrap();

        match locator.find_typedef_content(module) {
            TypedefLocatorResult::MissingTypedef { local, .. } => assert!(local),
            other => panic!("expected MissingTypedef after gate, got {:?}", other),
        }

        std::fs::write(&typedef_path, "// Generated\n").unwrap();
        let same = TypedefLocator::new(
            locator.deps().clone(),
            Some(project.path().to_path_buf()),
            Target::host(),
        );
        assert!(matches!(
            same.find_typedef_content(module),
            TypedefLocatorResult::Found { .. }
        ));

        std::fs::write(project.path().join("foo/lib.go"), "package lib // edited\n").unwrap();
        let edited = TypedefLocator::new(
            locator.deps().clone(),
            Some(project.path().to_path_buf()),
            Target::host(),
        );
        assert!(matches!(
            edited.find_typedef_content(module),
            TypedefLocatorResult::MissingTypedef { local: true, .. }
        ));
    }

    #[test]
    fn a_refreshed_locator_rereads_local_source_an_older_one_has_already_seen() {
        let module = "example.com/me/foo";
        let (project, locator) = local_project(module, "foo");

        let pkg = GoPackage {
            module: GoModule {
                path: module,
                version: "v0.0.0",
                replacement: Some(crate::ReplacementTarget::LocalDirectory),
            },
            package: module,
        };
        let typedef_path = pkg.typedef_path(&typedef_cache_dir(project.path()), Target::host());
        write_typedef(&typedef_path, "pub fn Old() -> int\n");
        let stamp = locator
            .local_stamp_value()
            .expect("a local module is declared");
        std::fs::write(typedef_path.with_file_name("foo.stamp"), stamp).unwrap();

        let is_offered = |locator: &TypedefLocator| {
            locator
                .importable_packages()
                .iter()
                .any(|package| package.path == module)
        };
        assert!(is_offered(&locator));

        std::fs::write(project.path().join("foo/lib.go"), "package lib // edited\n").unwrap();
        assert!(
            is_offered(&locator),
            "this locator answered once already and holds that answer"
        );
        assert!(
            !is_offered(&locator.with_fresh_local_stamp()),
            "a copy that has yet to look sees the edit, so a periodic re-walk \
             cannot keep serving what an earlier analysis saw"
        );
    }

    #[test]
    fn importable_packages_drop_a_local_module_whose_source_moved_on() {
        let module = "example.com/me/foo";
        let (project, locator) = local_project(module, "foo");

        let pkg = GoPackage {
            module: GoModule {
                path: module,
                version: "v0.0.0",
                replacement: Some(crate::ReplacementTarget::LocalDirectory),
            },
            package: module,
        };
        let typedef_path = pkg.typedef_path(&typedef_cache_dir(project.path()), Target::host());
        write_typedef(&typedef_path, "pub fn Old() -> int\n");
        let stamp = locator
            .local_stamp_value()
            .expect("a declared local module has a stamp");
        std::fs::write(typedef_path.with_file_name("foo.stamp"), stamp).unwrap();

        let is_offered = |locator: &TypedefLocator| {
            locator
                .importable_packages()
                .iter()
                .any(|package| package.path == module)
        };
        assert!(is_offered(&locator), "a stamped typedef is current");

        std::fs::write(project.path().join("foo/lib.go"), "package lib // edited\n").unwrap();
        let edited = TypedefLocator::new(
            locator.deps().clone(),
            Some(project.path().to_path_buf()),
            Target::host(),
        );
        assert!(
            !is_offered(&edited),
            "the typedef describes source that no longer exists, so completing from it \
             would suggest gone packages and insert an import that fails to analyse"
        );
    }
}
