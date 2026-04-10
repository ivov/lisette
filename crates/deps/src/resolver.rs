use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::manifest::GoDependency;

/// Result of looking up a Go typedef.
#[derive(Debug)]
pub enum GoTypedefResult {
    /// Found the typedef source.
    Found {
        source: Cow<'static, str>,
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
    ProjectOverride,
    Cache,
}

/// Resolves Go package import paths to their typedef sources.
///
/// Holds the dependency map from `lisette.toml`, the project root (for
/// overrides), and the home directory (for the global cache).
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

    /// Resolve a Go package path (without the `go:` prefix) to its typedef source.
    pub fn resolve(&self, go_pkg: &str) -> GoTypedefResult {
        if !has_domain(go_pkg) {
            return match stdlib::get_go_stdlib_typedef(go_pkg) {
                Some(source) => GoTypedefResult::Found {
                    source: Cow::Borrowed(source),
                    origin: TypedefOrigin::Stdlib,
                },
                None => GoTypedefResult::UnknownStdlib,
            };
        }

        let Some((module_path, dep)) = self.resolve_package_to_module(go_pkg) else {
            return GoTypedefResult::UndeclaredImport;
        };

        let version = &dep.version;

        for (path, origin) in self.typedef_search_paths(module_path, version, go_pkg) {
            match std::fs::read_to_string(&path) {
                Ok(source) => {
                    return GoTypedefResult::Found {
                        source: Cow::Owned(source),
                        origin,
                    };
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return GoTypedefResult::UnreadableTypedef {
                        path,
                        error: e.to_string(),
                    };
                }
                Err(_) => {} // NotFound — try next path
            }
        }

        GoTypedefResult::MissingTypedef {
            module: module_path.to_string(),
            version: version.clone(),
        }
    }

    /// Return the ordered list of paths to check for a third-party typedef.
    fn typedef_search_paths(
        &self,
        module_path: &str,
        version: &str,
        go_pkg: &str,
    ) -> Vec<(PathBuf, TypedefOrigin)> {
        let mut paths = Vec::with_capacity(2);

        if let Some(ref root) = self.project_root {
            paths.push((
                typedef_path_in(root.join(".lisette/deps/go"), module_path, version, go_pkg),
                TypedefOrigin::ProjectOverride,
            ));
        }

        if let Some(ref home) = self.home {
            paths.push((
                typedef_path_in(
                    PathBuf::from(home).join(".lisette/cache/go"),
                    module_path,
                    version,
                    go_pkg,
                ),
                TypedefOrigin::Cache,
            ));
        }

        paths
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

/// Determine the cache/override path for a typedef file.
///
/// Given a base directory (e.g. `~/.lisette/cache/go`), a module path, version,
/// and package path, return the full path to the `.d.lis` file.
///
/// Root package: `.../github.com/gorilla/mux@v1.8.0/mux.d.lis`
/// Subpackage:   `.../github.com/gorilla/mux@v1.8.0/middleware/auth/auth.d.lis`
pub fn typedef_path_in(
    base: PathBuf,
    module_path: &str,
    version: &str,
    package_path: &str,
) -> PathBuf {
    let module_dir = base.join(format!("{}@{}", module_path, version));

    let relative = if package_path == module_path {
        ""
    } else {
        package_path
            .strip_prefix(module_path)
            .and_then(|s| s.strip_prefix('/'))
            .unwrap_or("")
    };

    let last_segment = package_path.rsplit('/').next().unwrap_or(package_path);

    let filename = format!("{}.d.lis", last_segment);

    if relative.is_empty() {
        module_dir.join(filename)
    } else {
        module_dir.join(relative).join(&filename)
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

/// Compute the full cache path for a typedef. Convenience wrapper.
pub fn typedef_cache_path(module_path: &str, version: &str, package_path: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    typedef_path_in(
        Path::new(&home).join(".lisette/cache/go"),
        module_path,
        version,
        package_path,
    )
}
