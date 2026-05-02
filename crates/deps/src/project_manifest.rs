use std::collections::{BTreeMap, HashSet};
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

    let bytes = fs::read(&project_toml_path)
        .map_err(|_| format!("No `lisette.toml` manifest in `{}`", project_root.display()))?;
    let content =
        strip_bom_to_str(&bytes).map_err(|e| format!("Invalid `lisette.toml` manifest: {}", e))?;

    toml::from_str(content).map_err(|e| format!("Invalid `lisette.toml` manifest: {}", e))
}

pub fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project name is empty".to_string());
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~')))
    {
        return Err(format!(
            "`{}` contains `{}`, which is not allowed in a project name (only ASCII letters, digits, and `.-_~`)",
            name, bad
        ));
    }
    Ok(())
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

pub fn check_no_subpackage_deps(manifest: &Manifest) -> Result<(), String> {
    let deps = manifest.go_deps();
    let has_via = |d: &GoDependency| d.via.as_ref().is_some_and(|v| !v.is_empty());

    for (key, dep) in &deps {
        let Some((parent, parent_dep)) = deps
            .iter()
            .find(|(other, _)| other.as_str() != key.as_str() && is_pkg_under(key, other))
        else {
            continue;
        };

        if has_via(dep) || has_via(parent_dep) {
            continue;
        }

        return Err(format!(
            "`{}` in `[dependencies.go]` is a subpackage of `{}`; remove this entry and rely on the parent module pin",
            key, parent
        ));
    }

    Ok(())
}

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

fn strip_bom_to_str(bytes: &[u8]) -> Result<&str, std::str::Utf8Error> {
    let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    std::str::from_utf8(body)
}

struct ManifestEncoding {
    had_bom: bool,
    had_crlf: bool,
}

fn open_manifest(path: &Path) -> Result<(ManifestEncoding, toml_edit::DocumentMut), String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read `lisette.toml`: {}", e))?;
    let had_bom = bytes.starts_with(UTF8_BOM);
    let content =
        strip_bom_to_str(&bytes).map_err(|e| format!("Failed to read `lisette.toml`: {}", e))?;
    let had_crlf = content.contains("\r\n");
    let manifest: toml_edit::DocumentMut = content
        .parse()
        .map_err(|e| format!("Failed to parse `lisette.toml`: {}", e))?;
    Ok((ManifestEncoding { had_bom, had_crlf }, manifest))
}

fn save_manifest(
    path: &Path,
    encoding: &ManifestEncoding,
    manifest: &toml_edit::DocumentMut,
) -> Result<(), String> {
    let mut serialized = manifest.to_string();
    if encoding.had_crlf {
        serialized = serialized.replace('\n', "\r\n");
    }
    if encoding.had_bom {
        let mut out = Vec::with_capacity(UTF8_BOM.len() + serialized.len());
        out.extend_from_slice(UTF8_BOM);
        out.extend_from_slice(serialized.as_bytes());
        fs::write(path, out)
    } else {
        fs::write(path, serialized)
    }
    .map_err(|e| format!("Failed to write `lisette.toml`: {}", e))
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
    let path = project_root.join("lisette.toml");
    let (encoding, mut manifest) = open_manifest(&path)?;
    let go = ensure_go_deps_table(&mut manifest)?;

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

    save_manifest(&path, &encoding, &manifest)
}

pub fn remove_go_dep(project_root: &Path, go_dep_path: &str) -> Result<(), String> {
    let path = project_root.join("lisette.toml");
    let (encoding, mut manifest) = open_manifest(&path)?;

    if let Some(deps) = manifest
        .get_mut("dependencies")
        .and_then(|d| d.as_table_mut())
        && let Some(go) = deps.get_mut("go").and_then(|g| g.as_table_mut())
    {
        go.remove(go_dep_path);
    }

    save_manifest(&path, &encoding, &manifest)
}

/// Trimmed transitive dep. `removed_parents` are parents dropped from `via`.
pub struct TrimmedVia {
    pub module_path: String,
    pub removed_parents: Vec<String>,
}

pub struct ResolveReport {
    pub promoted: Vec<String>,
    pub removed: Vec<String>,
}

/// Drop `via` parents that are no longer manifest keys. Never deletes entries.
/// `resolve_empty_via` handles entries left with `via = []`.
pub fn trim_dead_via_parents(project_root: &Path) -> Result<Vec<TrimmedVia>, String> {
    let manifest = parse_manifest(project_root)?;
    let live_deps = manifest.go_deps();
    let live_paths: HashSet<&str> = live_deps.keys().map(|s| s.as_str()).collect();

    let mut trimmed = Vec::new();

    for (dep_path, dep) in &live_deps {
        let Some(ref via) = dep.via else { continue };

        let removed_parents: Vec<String> = via
            .iter()
            .filter(|parent| !live_paths.contains(parent.as_str()))
            .cloned()
            .collect();

        if removed_parents.is_empty() {
            continue;
        }

        let mut canonical: Vec<String> = via
            .iter()
            .filter(|parent| live_paths.contains(parent.as_str()))
            .cloned()
            .collect();
        canonical.sort();
        canonical.dedup();

        upsert_go_dep(project_root, dep_path, &dep.version, Some(canonical))?;
        trimmed.push(TrimmedVia {
            module_path: dep_path.clone(),
            removed_parents,
        });
    }

    Ok(trimmed)
}

/// For each entry with `via = []`, promote (drop the `via` field) if any
/// `imported_pkgs` path maps to it by longest-declared-prefix; otherwise
/// remove the entry.
///
/// Each import maps to a single best key — its longest declared prefix. E.g.
/// `k8s.io/api/core/v1` maps to `k8s.io/api` (not `k8s.io`) when both are
/// declared, preventing double-counting against nested keys.
pub fn resolve_empty_via(
    project_root: &Path,
    imported_pkgs: &[String],
) -> Result<ResolveReport, String> {
    let manifest = parse_manifest(project_root)?;
    let live_deps = manifest.go_deps();

    let mut matched: HashSet<String> = HashSet::new();
    for pkg in imported_pkgs {
        if let Some((module, _)) = find_module_for_pkg(&live_deps, pkg) {
            matched.insert(module.to_string());
        }
    }

    let mut promoted = Vec::new();
    let mut removed = Vec::new();

    for (dep_path, dep) in &live_deps {
        let Some(ref via) = dep.via else { continue };
        if !via.is_empty() {
            continue;
        }

        if matched.contains(dep_path.as_str()) {
            upsert_go_dep(project_root, dep_path, &dep.version, None)?;
            promoted.push(dep_path.clone());
        } else {
            remove_go_dep(project_root, dep_path)?;
            removed.push(dep_path.clone());
        }
    }

    Ok(ResolveReport { promoted, removed })
}

/// Whether `pkg_path` equals `module_path` or is a path nested under it
/// (`module_path` followed by `/`).
fn is_pkg_under(pkg_path: &str, module_path: &str) -> bool {
    pkg_path == module_path
        || (pkg_path.starts_with(module_path)
            && pkg_path.as_bytes().get(module_path.len()) == Some(&b'/'))
}

/// Longest declared module path that is a prefix of `pkg_path`, matching the
/// full key or a key followed by `/`.
pub(crate) fn find_module_for_pkg<'a>(
    deps: &'a BTreeMap<String, GoDependency>,
    pkg_path: &str,
) -> Option<(&'a str, &'a GoDependency)> {
    let mut best: Option<(&str, &GoDependency)> = None;
    for (module_path, dep) in deps {
        if is_pkg_under(pkg_path, module_path)
            && best
                .as_ref()
                .is_none_or(|(prev, _)| module_path.len() > prev.len())
        {
            best = Some((module_path.as_str(), dep));
        }
    }
    best
}

fn ensure_go_deps_table(
    manifest: &mut toml_edit::DocumentMut,
) -> Result<&mut toml_edit::Table, String> {
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
    deps["go"]
        .as_table_mut()
        .ok_or_else(|| "Invalid `lisette.toml`: `dependencies.go` is not a table".to_string())
}
