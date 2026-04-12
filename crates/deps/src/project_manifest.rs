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
    let project_toml_path = project_root.join("lisette.toml");

    let content = fs::read_to_string(&project_toml_path)
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

/// Add or update a Go dependency in `lisette.toml`.
///
/// ```text
/// "github.com/gorilla/mux" = "v1.8.0"
/// "github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux"] }
/// ```
pub fn upsert_go_dep(
    project_root: &Path,
    module_path: &str,
    version: &str,
    via: Option<Vec<String>>,
) -> Result<(), String> {
    let manifest_toml_path = project_root.join("lisette.toml");
    let manifest_content = fs::read_to_string(&manifest_toml_path)
        .map_err(|e| format!("Failed to read `lisette.toml`: {}", e))?;

    let mut manifest: toml_edit::DocumentMut = manifest_content
        .parse()
        .map_err(|e| format!("Failed to parse `lisette.toml`: {}", e))?;

    if manifest.get("dependencies").is_none() {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        manifest.insert("dependencies", toml_edit::Item::Table(table));
    }

    let deps = manifest["dependencies"]
        .as_table_mut()
        .ok_or("Invalid `lisette.toml`: `dependencies` is not a table")?;

    if deps.get("go").is_none() {
        deps.insert("go", toml_edit::Item::Table(toml_edit::Table::new()));
    }

    let go = deps["go"]
        .as_table_mut()
        .ok_or("Invalid `lisette.toml`: `dependencies.go` is not a table")?;

    match via {
        Some(mut via_list) => {
            via_list.sort();
            via_list.dedup();
            let mut inline = toml_edit::InlineTable::new();
            inline.insert("version", version.into());
            let mut arr = toml_edit::Array::new();
            for v in &via_list {
                arr.push(v.as_str());
            }
            inline.insert("via", toml_edit::Value::Array(arr));
            go.insert(
                module_path,
                toml_edit::value(toml_edit::Value::InlineTable(inline)),
            );
        }
        None => {
            go.insert(module_path, toml_edit::value(version));
        }
    }

    fs::write(&manifest_toml_path, manifest.to_string())
        .map_err(|e| format!("Failed to write `lisette.toml`: {}", e))?;

    Ok(())
}

pub fn remove_go_dep(project_root: &Path, go_dep_path: &str) -> Result<(), String> {
    let manifest_toml_path = project_root.join("lisette.toml");
    let manifest_content = fs::read_to_string(&manifest_toml_path)
        .map_err(|e| format!("Failed to read `lisette.toml`: {}", e))?;

    let mut manifest: toml_edit::DocumentMut = manifest_content
        .parse()
        .map_err(|e| format!("Failed to parse `lisette.toml`: {}", e))?;

    if let Some(deps) = manifest
        .get_mut("dependencies")
        .and_then(|d| d.as_table_mut())
        && let Some(go) = deps.get_mut("go").and_then(|g| g.as_table_mut())
    {
        go.remove(go_dep_path);
    }

    fs::write(&manifest_toml_path, manifest.to_string())
        .map_err(|e| format!("Failed to write `lisette.toml`: {}", e))?;

    Ok(())
}
