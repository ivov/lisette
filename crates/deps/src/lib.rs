mod project_manifest;
mod typedef_locator;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub use stdlib::Target;
use stdlib::{
    GO_STD_CONTENT_HASH, LIS_PRELUDE_SOURCE, PRELUDE_CONTENT_HASH, get_go_stdlib_packages,
    get_go_stdlib_typedef,
};

/// Disambiguates temp paths so concurrent typedef extractions do not collide.
static TYPEDEF_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

static STDLIB_EXTRACT_LOCK: Mutex<()> = Mutex::new(());

/// Test-only override for the home dir typedefs extract under.
static TYPEDEF_HOME: OnceLock<PathBuf> = OnceLock::new();

#[doc(hidden)]
pub fn set_typedef_home(home: PathBuf) {
    let _ = TYPEDEF_HOME.set(home);
}

fn typedef_home() -> Option<PathBuf> {
    match TYPEDEF_HOME.get() {
        Some(home) => Some(home.clone()),
        None => Some(PathBuf::from(std::env::var_os("HOME")?)),
    }
}

pub use project_manifest::{
    GoDependency, Manifest, ResolveReport, TrimmedVia, check_no_subpackage_deps,
    check_toolchain_version, parse_manifest, remove_go_dep, resolve_empty_via,
    trim_dead_via_parents, upsert_go_dependency, validate_project_name,
};
pub use typedef_locator::{
    Bindgen, BindgenFailure, BindgenGuard, BindgenSession, BindgenSetup, DeclarationStatus,
    TypedefLocator, TypedefLocatorResult, TypedefOrigin,
};

pub fn is_third_party(pkg: &str) -> bool {
    pkg.split('/')
        .next()
        .is_some_and(|first| first.contains('.'))
}

pub fn placeholder_require_version(module_path: &str) -> String {
    match path_major(module_path) {
        Some(major) => format!("v{}.0.0", major),
        None => "v0.0.0".to_string(),
    }
}

pub fn check_version_matches_path(module_path: &str, version: &str) -> Result<(), String> {
    let Some(actual) = version_major(version) else {
        return Ok(());
    };
    let ok = match path_major(module_path) {
        Some(required) => actual == required,
        None => actual <= 1,
    };
    if ok {
        return Ok(());
    }
    let expected = match path_major(module_path) {
        Some(required) => format!("v{}.x", required),
        None => "v0.x or v1.x".to_string(),
    };
    Err(format!(
        "version `{}` is not valid for module path `{}` (expected {})",
        version, module_path, expected
    ))
}

fn version_major(version: &str) -> Option<u64> {
    version
        .strip_prefix('v')?
        .split(['.', '-', '+'])
        .next()?
        .parse()
        .ok()
}

/// The major a path's suffix demands: `/vN` (N >= 2), or gopkg.in `.vN` (any N).
fn path_major(module_path: &str) -> Option<u64> {
    let last = module_path.rsplit('/').next()?;
    if module_path.starts_with("gopkg.in/") {
        let (_, digits) = last.rsplit_once(".v")?;
        return parse_major_digits(digits);
    }
    let digits = last.strip_prefix('v')?;
    parse_major_digits(digits).filter(|&major| major >= 2)
}

fn parse_major_digits(digits: &str) -> Option<u64> {
    let is_canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits.len() == 1 || !digits.starts_with('0'));
    is_canonical.then(|| digits.parse().ok()).flatten()
}

pub fn is_stdlib(pkg: &str) -> bool {
    !is_third_party(pkg)
}

pub fn typedef_cache_dir(project_root: &Path) -> PathBuf {
    let lis_version = env!("CARGO_PKG_VERSION");
    project_root
        .join("target/.lisette/typedefs")
        .join(format!("lis@v{}", lis_version))
}

/// Version dir for the materialized stdlib typedefs, under `~/.lisette/cache`.
/// The name encodes the compiler version and stdlib content hash, so its
/// existence proves the contents are current, and distinct versions or embedded
/// stdlibs get distinct dirs.
fn stdlib_typedef_version_dir() -> Option<PathBuf> {
    Some(
        typedef_home()?
            .join(".lisette/cache/stdlib-typedefs")
            .join(format!(
                "lis@v{}-{:016x}",
                env!("CARGO_PKG_VERSION"),
                GO_STD_CONTENT_HASH
            )),
    )
}

/// Deterministic on-disk path for a stdlib package's typedef. Pure path
/// construction; the files are written by [`ensure_stdlib_extracted`].
pub fn stdlib_typedef_path(target: Target, go_pkg: &str) -> Option<PathBuf> {
    Some(
        stdlib_typedef_version_dir()?
            .join(target.cache_segment())
            .join(format!("{go_pkg}.d.lis")),
    )
}

/// Materialize the whole embedded Go stdlib for `target` so the editor can open
/// typedefs as regular files. Runs once, at LSP startup.
///
/// All-or-nothing: written to a temp dir then atomically renamed, so the target
/// dir exists only when complete and one existence check gates the work. Sibling
/// `lis@v*` dirs from other versions are pruned.
pub fn ensure_stdlib_extracted(target: Target) {
    let Some(version_dir) = stdlib_typedef_version_dir() else {
        return;
    };
    let target_dir = version_dir.join(target.cache_segment());
    if target_dir.exists() {
        return;
    }

    let _guard = STDLIB_EXTRACT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if target_dir.exists() {
        return;
    }

    if std::fs::create_dir_all(&version_dir).is_err() {
        return;
    }
    clear_stale_temp_dirs(&version_dir, target, STALE_TEMP_AGE);

    // Build in a temp dir, then atomically rename into place. The temp name
    // carries the pid and a counter so concurrent extractions don't collide.
    let counter = TYPEDEF_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = version_dir.join(format!(
        "{}.tmp.{}.{}",
        target.cache_segment(),
        std::process::id(),
        counter
    ));
    if extract_all(&tmp, target).is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    if std::fs::rename(&tmp, &target_dir).is_err() {
        // Another extraction won the race, or the rename failed; drop our temp.
        let _ = std::fs::remove_dir_all(&tmp);
    }
    prune_stale_version_dirs(&version_dir);
}

/// Write every embedded stdlib typedef for `target` into `target_tmp`. Returns
/// `None` on the first I/O error so a partial set is never renamed into place.
fn extract_all(target_tmp: &Path, target: Target) -> Option<()> {
    for pkg in get_go_stdlib_packages(target) {
        let Some(source) = get_go_stdlib_typedef(pkg, target) else {
            continue;
        };
        let path = target_tmp.join(format!("{pkg}.d.lis"));
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, source).ok()?;
    }
    Some(())
}

const STALE_TEMP_AGE: Duration = Duration::from_secs(300);

fn clear_stale_temp_dirs(version_dir: &Path, target: Target, min_age: Duration) {
    let prefix = format!("{}.tmp.", target.cache_segment());
    let Ok(entries) = std::fs::read_dir(version_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name_matches = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix));
        if name_matches && temp_dir_is_stale(&entry, min_age) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn temp_dir_is_stale(entry: &std::fs::DirEntry, min_age: Duration) -> bool {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= min_age)
}

/// Remove sibling `lis@v*` dirs from other compiler versions or embedded stdlibs.
///
/// Caveat: if another `lis` version's LSP is running, this removes its dir; its
/// go-to-definition then declines until it restarts. Acceptable, normal lisette
/// users don't run multiple versions simultaneously.
fn prune_stale_version_dirs(current: &Path) {
    let Some(parent) = current.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current {
            continue;
        }
        let is_version_dir = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("lis@v"));
        if is_version_dir {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

fn prelude_typedef_version_dir() -> Option<PathBuf> {
    Some(
        typedef_home()?
            .join(".lisette/cache/prelude-typedefs")
            .join(format!(
                "lis@v{}-{:016x}",
                env!("CARGO_PKG_VERSION"),
                PRELUDE_CONTENT_HASH
            )),
    )
}

pub fn prelude_typedef_path() -> Option<PathBuf> {
    Some(prelude_typedef_version_dir()?.join("prelude.d.lis"))
}

pub fn is_generated_typedef_path(path: &Path) -> bool {
    let Some(home) = typedef_home() else {
        return false;
    };
    let cache = home.join(".lisette/cache");
    path.starts_with(cache.join("prelude-typedefs"))
        || path.starts_with(cache.join("stdlib-typedefs"))
}

pub fn ensure_prelude_extracted() {
    let Some(version_dir) = prelude_typedef_version_dir() else {
        return;
    };
    let path = version_dir.join("prelude.d.lis");
    if path.exists() {
        return;
    }
    if std::fs::create_dir_all(&version_dir).is_err() {
        return;
    }

    let counter = TYPEDEF_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = version_dir.join(format!("prelude.tmp.{}.{}", std::process::id(), counter));
    if std::fs::write(&tmp, LIS_PRELUDE_SOURCE).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    prune_stale_version_dirs(&version_dir);
}

/// The target of a replace directive: code from `path` at `version`.
#[derive(Clone, Copy)]
pub struct Replacement<'a> {
    pub path: &'a str,
    pub version: &'a str,
}

#[derive(Clone, Copy)]
pub struct GoModule<'a> {
    /// Module path, e.g. `github.com/gorilla/mux`. For a replaced module, the original path.
    pub path: &'a str,
    /// Version handed to Go commands. For a replaced module, a placeholder.
    pub version: &'a str,
    pub replacement: Option<Replacement<'a>>,
}

/// A Go package within a module.
pub struct GoPackage<'a> {
    /// The module that contains this package.
    pub module: GoModule<'a>,
    /// Package import path, either identical to `module.path` for the root package,
    /// or extended for subpackages (e.g. `github.com/gorilla/mux/middleware`).
    pub package: &'a str,
}

impl GoPackage<'_> {
    /// Build the path to a `.d.lis` file under a base directory.
    ///
    /// ```text
    /// <project>/target/.lisette/typedefs/lis@v0.1.6/darwin_arm64/github.com/gorilla/mux@v1.8.0/mux.d.lis
    /// <project>/target/.lisette/typedefs/lis@v0.1.6/darwin_arm64/github.com/gorilla/mux@v1.8.0/middleware/middleware.d.lis
    /// ```
    pub fn typedef_path(&self, base_dir: &Path, target: Target) -> PathBuf {
        let target_dir = base_dir.join(target.cache_segment());
        let module_dir = match self.module.replacement {
            Some(replacement) => target_dir
                .join("replaced")
                .join(self.module.path)
                .join("module")
                .join(format!("{}@{}", replacement.path, replacement.version)),
            None => target_dir.join(format!("{}@{}", self.module.path, self.module.version)),
        };

        let relative = if self.package == self.module.path {
            ""
        } else {
            self.package
                .strip_prefix(self.module.path)
                .and_then(|s| s.strip_prefix('/'))
                .unwrap_or("")
        };

        let last_segment = self.package.rsplit('/').next().unwrap_or(self.package);

        let filename = format!("{}.d.lis", last_segment);

        if relative.is_empty() {
            module_dir.join(filename)
        } else {
            module_dir.join(relative).join(&filename)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_stale_temp_dirs_removes_old_temps_but_keeps_fresh_and_other_targets() {
        let version_dir = tempfile::tempdir().unwrap();
        let root = version_dir.path();
        let target = Target::new("darwin", "arm64");

        let this_target = root.join("darwin_arm64.tmp.999.0");
        std::fs::create_dir_all(&this_target).unwrap();
        std::fs::write(this_target.join("fmt.d.lis"), "x").unwrap();
        let completed = root.join("darwin_arm64");
        std::fs::create_dir_all(&completed).unwrap();
        let other_target_tmp = root.join("linux_amd64.tmp.1.0");
        std::fs::create_dir_all(&other_target_tmp).unwrap();

        clear_stale_temp_dirs(root, target, STALE_TEMP_AGE);
        assert!(this_target.exists(), "a fresh in-flight temp is kept");

        clear_stale_temp_dirs(root, target, Duration::ZERO);
        assert!(
            !this_target.exists(),
            "a stale temp of this target is removed"
        );
        assert!(completed.exists(), "the completed target dir is kept");
        assert!(other_target_tmp.exists(), "another target's temp is kept");
    }

    #[test]
    fn typedef_path_uses_replaced_layout_keyed_by_replacement_identity() {
        let target = Target::new("linux", "amd64");
        let pkg = GoPackage {
            module: GoModule {
                path: "example.com/dragon",
                version: "v0.0.0",
                replacement: Some(Replacement {
                    path: "fork.example/dragon",
                    version: "v1.2.0",
                }),
            },
            package: "example.com/dragon/server",
        };
        let path = pkg.typedef_path(Path::new("/base"), target);
        assert_eq!(
            path,
            Path::new(
                "/base/linux_amd64/replaced/example.com/dragon/module/fork.example/dragon@v1.2.0/server/server.d.lis"
            )
        );
    }

    #[test]
    fn check_version_matches_path_enforces_major_suffix() {
        // /vN path requires vN
        assert!(check_version_matches_path("example.com/lib/v2", "v2.3.0").is_ok());
        assert!(check_version_matches_path("example.com/lib/v2", "v1.0.0").is_err());
        // gopkg.in .vN path requires vN
        assert!(check_version_matches_path("gopkg.in/yaml.v3", "v3.1.0").is_ok());
        assert!(check_version_matches_path("gopkg.in/yaml.v3", "v2.0.0").is_err());
        // plain path requires v0/v1 (incl. pseudo-versions)
        assert!(check_version_matches_path("github.com/you/mux", "v1.5.0").is_ok());
        assert!(
            check_version_matches_path("github.com/you/mux", "v0.0.0-20260101000000-abc").is_ok()
        );
        assert!(check_version_matches_path("github.com/you/mux", "v3.1.0").is_err());
    }

    #[test]
    fn synthetic_require_version_is_major_matched() {
        assert_eq!(
            placeholder_require_version("github.com/df-mc/dragonfly"),
            "v0.0.0"
        );
        assert_eq!(placeholder_require_version("example.com/lib/v2"), "v2.0.0");
        assert_eq!(
            placeholder_require_version("example.com/lib/v10"),
            "v10.0.0"
        );
        assert_eq!(placeholder_require_version("example.com/lib/v1"), "v0.0.0");
        assert_eq!(placeholder_require_version("example.com/vault"), "v0.0.0");
        assert_eq!(placeholder_require_version("gopkg.in/yaml.v2"), "v2.0.0");
        assert_eq!(
            placeholder_require_version("gopkg.in/user/pkg.v3"),
            "v3.0.0"
        );
    }

    #[test]
    fn is_generated_typedef_path_matches_only_cache_files() {
        let Some(home) = typedef_home() else {
            return;
        };
        let cache = home.join(".lisette/cache");

        assert!(is_generated_typedef_path(
            &cache.join("prelude-typedefs/lis@v1-abc/prelude.d.lis")
        ));
        assert!(is_generated_typedef_path(
            &cache.join("stdlib-typedefs/lis@v1-abc/darwin_arm64/fmt.d.lis")
        ));
        assert!(!is_generated_typedef_path(
            &home.join("project/src/main.lis")
        ));
        assert!(!is_generated_typedef_path(Path::new("/etc/passwd")));
    }
}
