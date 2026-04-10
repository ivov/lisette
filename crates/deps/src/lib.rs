mod manifest;
mod resolver;

pub use manifest::{GoDependency, Manifest, check_toolchain_version, parse_manifest};
pub use resolver::{GoDepResolver, GoTypedefResult, TypedefOrigin, has_domain, typedef_cache_path};
