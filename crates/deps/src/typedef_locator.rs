use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::project_manifest::{
    GoDependency, Manifest, check_toolchain_version, find_module_for_pkg, parse_manifest,
};
use crate::{GoModule, GoPackage, typedef_cache_dir};

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

#[derive(Debug, Clone, Default)]
pub struct TypedefLocator {
    deps: BTreeMap<String, GoDependency>,
    project_root: Option<PathBuf>,
    home: Option<String>,
}

impl TypedefLocator {
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
        let (_, locator) = Self::from_project_with_manifest(project_root)?;
        Ok(locator)
    }

    pub fn from_project_with_manifest(project_root: &Path) -> Result<(Manifest, Self), String> {
        let manifest = parse_manifest(project_root)?;

        check_toolchain_version(&manifest)?;

        let locator = Self::new(
            manifest.go_deps(),
            Some(project_root.to_path_buf()),
            std::env::var("HOME").ok(),
        );

        Ok((manifest, locator))
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    pub fn deps(&self) -> &BTreeMap<String, GoDependency> {
        &self.deps
    }

    pub fn is_declared_go_dep(&self, package_path: &str) -> bool {
        find_module_for_pkg(&self.deps, package_path).is_some()
    }

    /// Returns the `.d.lis` content for a Go package (without `go:` prefix).
    /// Checks embedded stdlib typedefs first, then the on-disk cache.
    pub fn find_typedef_content(&self, package_path: &str) -> TypedefLocatorResult {
        if crate::is_stdlib(package_path) {
            return match stdlib::get_go_stdlib_typedef(package_path) {
                Some(source) => TypedefLocatorResult::Found {
                    content: Cow::Borrowed(source),
                    origin: TypedefOrigin::Stdlib,
                },
                None => TypedefLocatorResult::UnknownStdlib,
            };
        }

        let Some((module_path, dep)) = find_module_for_pkg(&self.deps, package_path) else {
            return TypedefLocatorResult::UndeclaredImport;
        };

        let version = &dep.version;

        let Some(home_path) = &self.home else {
            return TypedefLocatorResult::MissingTypedef {
                module: module_path.to_string(),
                version: version.clone(),
            };
        };

        let pkg = GoPackage {
            module: GoModule {
                path: module_path,
                version,
            },
            package: package_path,
        };
        let typedef_cache_dir = typedef_cache_dir(home_path);
        let typedef_path = pkg.typedef_path(&typedef_cache_dir);

        match std::fs::read_to_string(&typedef_path) {
            Ok(source) => TypedefLocatorResult::Found {
                content: Cow::Owned(source),
                origin: TypedefOrigin::Cache,
            },
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                TypedefLocatorResult::UnreadableTypedef {
                    path: typedef_path,
                    error: e.to_string(),
                }
            }
            Err(_) => TypedefLocatorResult::MissingTypedef {
                module: module_path.to_string(),
                version: version.clone(),
            },
        }
    }
}
