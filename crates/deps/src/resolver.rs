use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::manifest::GoDependency;

/// Result of looking up a Go typedef.
#[derive(Debug)]
pub enum GoTypedefResult {
    Found {
        content: Cow<'static, str>,
        origin: TypedefOrigin,
    },
    /// Looks like a stdlib package but no stdlib typedef exists.
    UnknownStdlib,
    /// Has a domain-style path but is not declared in the manifest.
    UndeclaredImport,
    /// Declared in the manifest but no `.d.lis` file found on disk.
    MissingTypedef { module: String, version: String },
    /// Typedef file exists but could not be read.
    UnreadableTypedef { path: PathBuf, error: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedefOrigin {
    Stdlib,
    Cache,
}

/// Resolves Go package import paths to their typedef sources.
///
/// Holds the dependency map from `lisette.toml` and the home directory
/// (for the global cache).
#[derive(Debug, Clone, Default)]
pub struct GoDepResolver {
    deps: BTreeMap<String, GoDependency>,
    project_root: Option<PathBuf>,
    home: Option<String>,
}

impl GoDepResolver {
    pub fn new(
        deps: BTreeMap<String, GoDependency>,
        project_root: Option<PathBuf>,
        home: Option<String>,
    ) -> Self {
        Self {
            deps,
            project_root,
            home,
        }
    }

    pub fn from_project(project_root: &Path) -> Result<Self, String> {
        let (_, resolver) = Self::from_project_with_manifest(project_root)?;
        Ok(resolver)
    }

    pub fn from_project_with_manifest(
        project_root: &Path,
    ) -> Result<(crate::Manifest, Self), String> {
        let manifest = crate::parse_manifest(project_root)?;
        crate::check_toolchain_version(&manifest)?;
        let resolver = Self::new(
            manifest.go_deps(),
            Some(project_root.to_path_buf()),
            std::env::var("HOME").ok(),
        );
        Ok((manifest, resolver))
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    pub fn deps(&self) -> &BTreeMap<String, GoDependency> {
        &self.deps
    }

    pub fn has_deps(&self) -> bool {
        !self.deps.is_empty()
    }

    /// Returns the `.d.lis` content for a Go package (without `go:` prefix).
    /// Checks embedded stdlib typedefs first, then the on-disk cache.
    pub fn find_typedef_content(&self, go_pkg: &str) -> GoTypedefResult {
        if !has_domain(go_pkg) {
            return match stdlib::get_go_stdlib_typedef(go_pkg) {
                Some(source) => GoTypedefResult::Found {
                    content: Cow::Borrowed(source),
                    origin: TypedefOrigin::Stdlib,
                },
                None => GoTypedefResult::UnknownStdlib,
            };
        }

        let Some((module_path, dep)) = self.resolve_package_to_module(go_pkg) else {
            return GoTypedefResult::UndeclaredImport;
        };

        let version = &dep.version;

        let Some(home_path) = &self.home else {
            return GoTypedefResult::MissingTypedef {
                module: module_path.to_string(),
                version: version.clone(),
            };
        };

        let pkg_ref = GoPackageRef {
            module_path,
            version,
            package_path: go_pkg,
        };
        let typedef_cache_dir = typedef_cache_dir(home_path);
        let typedef_path = pkg_ref.build_typedef_path(&typedef_cache_dir);

        match std::fs::read_to_string(&typedef_path) {
            Ok(source) => GoTypedefResult::Found {
                content: Cow::Owned(source),
                origin: TypedefOrigin::Cache,
            },
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                GoTypedefResult::UnreadableTypedef {
                    path: typedef_path,
                    error: e.to_string(),
                }
            }
            Err(_) => GoTypedefResult::MissingTypedef {
                module: module_path.to_string(),
                version: version.clone(),
            },
        }
    }

    /// Find the longest declared module path that is a prefix of the package path.
    fn resolve_package_to_module(&self, package_path: &str) -> Option<(&str, &GoDependency)> {
        let mut best: Option<(&str, &GoDependency)> = None;

        for (module_path, dep) in &self.deps {
            let is_match = package_path == module_path.as_str()
                || (package_path.starts_with(module_path.as_str())
                    && package_path.as_bytes().get(module_path.len()) == Some(&b'/'));

            if is_match
                && best
                    .as_ref()
                    .is_none_or(|(prev, _)| module_path.len() > prev.len())
            {
                best = Some((module_path.as_str(), dep));
            }
        }

        best
    }
}

pub struct GoPackageRef<'a> {
    /// Module path, e.g. `github.com/gorilla/mux`.
    pub module_path: &'a str,
    /// Module version, e.g. `v1.8.0`.
    pub version: &'a str,
    /// Package import path, either identical to `module_path` for the root package,
    /// or extended for subpackages (e.g. `github.com/gorilla/mux/middleware`).
    pub package_path: &'a str,
}

impl GoPackageRef<'_> {
    /// Build the path to a `.d.lis` file under a base directory.
    ///
    /// ```text
    /// ~/.lisette/cache/typedefs/lis@v0.1.6/github.com/gorilla/mux@v1.8.0/mux.d.lis
    /// ~/.lisette/cache/typedefs/lis@v0.1.6/github.com/gorilla/mux@v1.8.0/middleware/middleware.d.lis
    /// ```
    pub fn build_typedef_path(&self, base: &Path) -> PathBuf {
        let module_dir = base.join(format!("{}@{}", self.module_path, self.version));

        let relative = if self.package_path == self.module_path {
            ""
        } else {
            self.package_path
                .strip_prefix(self.module_path)
                .and_then(|s| s.strip_prefix('/'))
                .unwrap_or("")
        };

        let last_segment = self
            .package_path
            .rsplit('/')
            .next()
            .unwrap_or(self.package_path);

        let filename = format!("{}.d.lis", last_segment);

        if relative.is_empty() {
            module_dir.join(filename)
        } else {
            module_dir.join(relative).join(&filename)
        }
    }
}

/// A Go package path has a domain if its first segment contains a dot.
/// This is the canonical stdlib vs third-party distinction: stdlib paths
/// like `net/http` or `fmt` never have dots in the first segment, while
/// third-party paths like `github.com/gorilla/mux` always do.
pub fn has_domain(pkg: &str) -> bool {
    pkg.split('/')
        .next()
        .is_some_and(|first| first.contains('.'))
}

pub fn typedef_cache_dir(home: &str) -> PathBuf {
    let lis_version = env!("CARGO_PKG_VERSION");
    PathBuf::from(home).join(format!(".lisette/cache/typedefs/lis@v{}", lis_version))
}
