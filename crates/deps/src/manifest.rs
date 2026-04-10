use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub project: Project,
    pub toolchain: Option<Toolchain>,
    pub dependencies: Option<Dependencies>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Toolchain {
    pub lis: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dependencies {
    #[serde(default)]
    pub go: BTreeMap<String, GoDependency>,
}

#[derive(Debug, Clone)]
pub struct GoDependency {
    pub version: String,
    pub via: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for GoDependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct GoDependencyVisitor;

        impl<'de> Visitor<'de> for GoDependencyVisitor {
            type Value = GoDependency;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a version string or a table with `version` and optional `via`")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<GoDependency, E> {
                Ok(GoDependency {
                    version: v.to_string(),
                    via: None,
                })
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<GoDependency, M::Error> {
                let mut version = None;
                let mut via = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "version" => version = Some(map.next_value()?),
                        "via" => via = Some(map.next_value()?),
                        other => {
                            return Err(de::Error::unknown_field(other, &["version", "via"]));
                        }
                    }
                }

                let version = version.ok_or_else(|| de::Error::missing_field("version"))?;

                Ok(GoDependency { version, via })
            }
        }

        deserializer.deserialize_any(GoDependencyVisitor)
    }
}

impl Manifest {
    pub fn go_deps(&self) -> BTreeMap<String, GoDependency> {
        self.dependencies
            .as_ref()
            .map(|d| d.go.clone())
            .unwrap_or_default()
    }
}

pub fn parse_manifest(project_root: &Path) -> Result<Manifest, String> {
    let toml_path = project_root.join("lisette.toml");

    let content = fs::read_to_string(&toml_path)
        .map_err(|_| format!("No `lisette.toml` manifest in `{}`", project_root.display()))?;

    toml::from_str(&content).map_err(|e| format!("Invalid `lisette.toml` manifest: {}", e))
}

pub fn check_toolchain_version(manifest: &Manifest) -> Result<(), String> {
    let Some(ref toolchain) = manifest.toolchain else {
        return Ok(());
    };

    let running = env!("CARGO_PKG_VERSION");
    if running != toolchain.lis {
        return Err(format!(
            "Toolchain mismatch: `lisette.toml` pins lis {} but running lis {}",
            toolchain.lis, running,
        ));
    }

    Ok(())
}
