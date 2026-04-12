mod project_manifest;
mod typedef_locator;

use std::path::{Path, PathBuf};

pub use project_manifest::{
    GoDependency, Manifest, check_toolchain_version, parse_manifest, remove_go_dep, upsert_go_dep,
    validate_project_name,
};
pub use typedef_locator::{TypedefLocator, TypedefLocatorResult, TypedefOrigin};

pub fn is_third_party(pkg: &str) -> bool {
    pkg.split('/')
        .next()
        .is_some_and(|first| first.contains('.'))
}

pub fn is_stdlib(pkg: &str) -> bool {
    !is_third_party(pkg)
}

pub fn typedef_cache_dir(home: &str) -> PathBuf {
    let lis_version = env!("CARGO_PKG_VERSION");
    PathBuf::from(home).join(format!(".lisette/cache/typedefs/lis@v{}", lis_version))
}

#[derive(Clone, Copy)]
pub struct GoModule<'a> {
    /// Module path, e.g. `github.com/gorilla/mux`.
    pub path: &'a str,
    /// Module version, e.g. `v1.8.0`.
    pub version: &'a str,
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
    /// ~/.lisette/cache/typedefs/lis@v0.1.6/github.com/gorilla/mux@v1.8.0/mux.d.lis
    /// ~/.lisette/cache/typedefs/lis@v0.1.6/github.com/gorilla/mux@v1.8.0/middleware/middleware.d.lis
    /// ```
    pub fn typedef_path(&self, base_dir: &Path) -> PathBuf {
        let module_dir = base_dir.join(format!("{}@{}", self.module.path, self.module.version));

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
