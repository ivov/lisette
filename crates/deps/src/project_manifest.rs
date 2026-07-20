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
pub enum GoDependency {
    Remote {
        version: String,
        via: Option<Vec<String>>,
    },
    Replaced {
        replacement_path: String,
        replacement_version: String,
        via: Option<Vec<String>>,
    },
}

impl GoDependency {
    pub fn via(&self) -> Option<&[String]> {
        match self {
            GoDependency::Remote { via, .. } | GoDependency::Replaced { via, .. } => via.as_deref(),
        }
    }

    pub fn with_via(&self, via: Option<Vec<String>>) -> GoDependency {
        match self {
            GoDependency::Remote { version, .. } => GoDependency::Remote {
                version: version.clone(),
                via,
            },
            GoDependency::Replaced {
                replacement_path,
                replacement_version,
                ..
            } => GoDependency::Replaced {
                replacement_path: replacement_path.clone(),
                replacement_version: replacement_version.clone(),
                via,
            },
        }
    }
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
                f.write_str(
                    "a version string, a table with `version` (and optional `via`), or a table with `replace` (and optional `via`)",
                )
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<GoDependency, E> {
                Ok(GoDependency::Remote {
                    version: v.to_string(),
                    via: None,
                })
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<GoDependency, M::Error> {
                let mut version: Option<String> = None;
                let mut replacement: Option<String> = None;
                let mut via: Option<Vec<String>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "version" => version = Some(map.next_value()?),
                        "replacement" => replacement = Some(map.next_value()?),
                        "via" => via = Some(map.next_value()?),
                        other => {
                            return Err(de::Error::unknown_field(
                                other,
                                &["version", "replacement", "via"],
                            ));
                        }
                    }
                }

                match (version, replacement) {
                    (Some(version), None) => Ok(GoDependency::Remote { version, via }),
                    (None, Some(replacement)) => {
                        let (replacement_path, replacement_version) =
                            split_replacement(&replacement).map_err(de::Error::custom)?;
                        Ok(GoDependency::Replaced {
                            replacement_path,
                            replacement_version,
                            via,
                        })
                    }
                    (Some(_), Some(_)) => Err(de::Error::custom(
                        "a Go dependency cannot set both `version` and `replacement`",
                    )),
                    (None, None) => Err(de::Error::custom(
                        "a Go dependency table needs either `version` or `replacement`",
                    )),
                }
            }
        }

        deserializer.deserialize_any(GoDependencyVisitor)
    }
}

/// Split a `replacement` value of the form `<module-path>@<version>` into its parts.
fn split_replacement(replacement: &str) -> Result<(String, String), String> {
    let err = || {
        format!(
            "`replacement` must be of the form `<module-path>@<version>`, got `{}`",
            replacement
        )
    };
    let (path, version) = replacement.rsplit_once('@').ok_or_else(err)?;
    if path.is_empty() || version.is_empty() {
        return Err(err());
    }
    Ok((path.to_string(), version.to_string()))
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

    let manifest: Manifest =
        toml::from_str(content).map_err(|e| format!("Invalid `lisette.toml` manifest: {}", e))?;
    validate_go_dep_paths(&manifest)?;
    Ok(manifest)
}

/// A replaced entry's key (the original module) and its `replace` target must
/// both be third-party module paths, or resolution silently misclassifies them.
fn validate_go_dep_paths(manifest: &Manifest) -> Result<(), String> {
    for (key, dep) in &manifest.go_deps() {
        let GoDependency::Replaced {
            replacement_path, ..
        } = dep
        else {
            continue;
        };
        if !crate::is_third_party(key) {
            return Err(format!(
                "`{}` in `[dependencies.go]` has a `replace` but is not a third-party module path (its first path segment needs a dot)",
                key
            ));
        }
        if !crate::is_third_party(replacement_path) {
            return Err(format!(
                "the `replace` target `{}` for `{}` is not a third-party module path",
                replacement_path, key
            ));
        }
    }
    Ok(())
}

pub fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project name is empty".to_string());
    }
    if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
        return Err(format!(
            "`{}` has an empty path element (no leading, trailing, or doubled `/`)",
            name
        ));
    }
    for element in name.split('/') {
        if let Some(bad) = element
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~')))
        {
            return Err(format!(
                "`{}` contains `{}`, which is not allowed in a project name (only ASCII letters, digits, `.-_~`, and `/` between path elements)",
                name, bad
            ));
        }
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
    let has_via = |d: &GoDependency| d.via().is_some_and(|v| !v.is_empty());

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

/// Add or update a Go dependency in `lisette.toml`, written in the shape matching its variant.
pub fn upsert_go_dependency(
    project_root: &Path,
    module_path: &str,
    dep: &GoDependency,
) -> Result<(), String> {
    let path = project_root.join("lisette.toml");
    let (encoding, mut manifest) = open_manifest(&path)?;
    let go = ensure_go_deps_table(&mut manifest)?;

    let via = dep.via().map(|via_list| {
        let mut sorted = via_list.to_vec();
        sorted.sort();
        sorted.dedup();
        sorted
    });

    match dep {
        GoDependency::Remote { version, .. } => match via {
            Some(via_list) => {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("version", version.as_str().into());
                inline.insert("via", via_array(&via_list));
                go.insert(
                    module_path,
                    toml_edit::value(toml_edit::Value::InlineTable(inline)),
                );
            }
            None => {
                go.insert(module_path, toml_edit::value(version.as_str()));
            }
        },
        GoDependency::Replaced {
            replacement_path,
            replacement_version,
            ..
        } => {
            let mut inline = toml_edit::InlineTable::new();
            inline.insert(
                "replacement",
                format!("{}@{}", replacement_path, replacement_version)
                    .as_str()
                    .into(),
            );
            if let Some(via_list) = via {
                inline.insert("via", via_array(&via_list));
            }
            go.insert(
                module_path,
                toml_edit::value(toml_edit::Value::InlineTable(inline)),
            );
        }
    }

    save_manifest(&path, &encoding, &manifest)
}

fn via_array(via_list: &[String]) -> toml_edit::Value {
    let mut arr = toml_edit::Array::new();
    for v in via_list {
        arr.push(v.as_str());
    }
    toml_edit::Value::Array(arr)
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
        let Some(via) = dep.via() else { continue };

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

        upsert_go_dependency(project_root, dep_path, &dep.with_via(Some(canonical)))?;
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
        let Some(via) = dep.via() else { continue };
        if !via.is_empty() {
            continue;
        }

        if matched.contains(dep_path.as_str()) {
            upsert_go_dependency(project_root, dep_path, &dep.with_via(None))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn project_with(manifest: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lisette.toml"), manifest).unwrap();
        dir
    }

    fn manifest_text(dir: &TempDir) -> String {
        std::fs::read_to_string(dir.path().join("lisette.toml")).unwrap()
    }

    #[test]
    fn promotes_transitive_still_imported() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux"] }
"#,
        );

        trim_dead_via_parents(dir.path()).unwrap();
        let report =
            resolve_empty_via(dir.path(), &["github.com/gorilla/context".to_string()]).unwrap();

        assert_eq!(report.promoted, vec!["github.com/gorilla/context"]);
        let after = manifest_text(&dir);
        assert!(after.contains(r#""github.com/gorilla/context" = "v1.1.1""#));
        assert!(!after.contains("via"));
    }

    #[test]
    fn removes_transitive_no_longer_imported() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux"] }
"#,
        );

        trim_dead_via_parents(dir.path()).unwrap();
        let report = resolve_empty_via(dir.path(), &[]).unwrap();

        assert_eq!(report.removed, vec!["github.com/gorilla/context"]);
        assert!(!manifest_text(&dir).contains("gorilla/context"));
    }

    #[test]
    fn keeps_transitive_with_remaining_parents() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/mux" = "v1.8.0"
"github.com/gorilla/context" = { version = "v1.1.1", via = ["github.com/gorilla/mux", "github.com/old/dead"] }
"#,
        );

        trim_dead_via_parents(dir.path()).unwrap();
        resolve_empty_via(dir.path(), &[]).unwrap();

        let after = manifest_text(&dir);
        assert!(after.contains("gorilla/context"));
        assert!(after.contains("gorilla/mux"));
        assert!(!after.contains("old/dead"));
    }

    #[test]
    fn promotes_subpackage_via_longest_prefix() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"k8s.io/api" = { version = "v0.30.0", via = ["k8s.io/client-go"] }
"#,
        );

        trim_dead_via_parents(dir.path()).unwrap();
        let report = resolve_empty_via(dir.path(), &["k8s.io/api/core/v1".to_string()]).unwrap();

        assert_eq!(report.promoted, vec!["k8s.io/api"]);
        assert!(manifest_text(&dir).contains(r#""k8s.io/api" = "v0.30.0""#));
    }

    #[test]
    fn no_op_on_clean_manifest_is_byte_identical() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/mux" = "v1.8.0"
"#,
        );
        let before = manifest_text(&dir);

        let trimmed = trim_dead_via_parents(dir.path()).unwrap();
        let report =
            resolve_empty_via(dir.path(), &["github.com/gorilla/mux".to_string()]).unwrap();

        assert!(trimmed.is_empty());
        assert!(report.promoted.is_empty());
        assert!(report.removed.is_empty());
        assert_eq!(before, manifest_text(&dir));
    }

    #[test]
    fn find_module_for_pkg_picks_longest_declared_prefix() {
        let mut deps = BTreeMap::new();
        deps.insert(
            "k8s.io".to_string(),
            GoDependency::Remote {
                version: "v0.0.0".to_string(),
                via: None,
            },
        );
        deps.insert(
            "k8s.io/api".to_string(),
            GoDependency::Remote {
                version: "v0.30.0".to_string(),
                via: None,
            },
        );

        let (module, _) = find_module_for_pkg(&deps, "k8s.io/api/core/v1").unwrap();
        assert_eq!(module, "k8s.io/api");
        assert!(find_module_for_pkg(&deps, "example.com/other").is_none());
    }

    #[test]
    fn rejects_subpackage_dependency_with_clear_message() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"github.com/gorilla/mux" = "v1.8.0"
"github.com/gorilla/mux/middleware" = "v1.8.0"
"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();

        let error = check_no_subpackage_deps(&manifest).unwrap_err();
        assert!(error.contains("`github.com/gorilla/mux/middleware`"));
        assert!(error.contains("subpackage of `github.com/gorilla/mux`"));
    }

    fn replacement_manifest(entry: &str) -> TempDir {
        project_with(&format!(
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n{}\n",
            entry
        ))
    }

    #[test]
    fn parses_replacement_entry() {
        let dir = replacement_manifest(
            r#""github.com/df-mc/dragonfly" = { replacement = "github.com/fork/dragonfly@v1.2.0" }"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();
        match &manifest.go_deps()["github.com/df-mc/dragonfly"] {
            GoDependency::Replaced {
                replacement_path,
                replacement_version,
                via,
            } => {
                assert_eq!(replacement_path, "github.com/fork/dragonfly");
                assert_eq!(replacement_version, "v1.2.0");
                assert!(via.is_none());
            }
            other => panic!("expected Replaced, got {:?}", other),
        }
    }

    #[test]
    fn parses_replacement_with_via() {
        let dir = replacement_manifest(
            r#""github.com/df-mc/dragonfly" = { replacement = "github.com/fork/dragonfly@v0.0.0-20260101000000-abcdef123456", via = ["github.com/x/y"] }"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();
        match &manifest.go_deps()["github.com/df-mc/dragonfly"] {
            GoDependency::Replaced {
                replacement_version,
                via,
                ..
            } => {
                assert_eq!(replacement_version, "v0.0.0-20260101000000-abcdef123456");
                assert_eq!(
                    via.as_deref(),
                    Some(["github.com/x/y".to_string()].as_slice())
                );
            }
            other => panic!("expected Replaced, got {:?}", other),
        }
    }

    #[test]
    fn rejects_replacement_without_version() {
        let dir = replacement_manifest(
            r#""github.com/df-mc/dragonfly" = { replacement = "github.com/fork/dragonfly" }"#,
        );
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(error.contains("<module-path>@<version>"), "{}", error);
    }

    #[test]
    fn rejects_replacement_with_non_third_party_key() {
        let dir =
            replacement_manifest(r#""dragon" = { replacement = "fork.example/dragon@v1.2.0" }"#);
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(error.contains("not a third-party module path"), "{}", error);
    }

    #[test]
    fn rejects_replacement_with_non_third_party_replacement_path() {
        let dir =
            replacement_manifest(r#""example.com/dragon" = { replacement = "localfork@v1.2.0" }"#);
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(
            error.contains("`localfork`") && error.contains("not a third-party"),
            "{}",
            error
        );
    }

    #[test]
    fn rejects_both_version_and_replacement() {
        let dir = replacement_manifest(
            r#""github.com/df-mc/dragonfly" = { version = "v1.0.0", replacement = "github.com/fork/dragonfly@v1.2.0" }"#,
        );
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(
            error.contains("both `version` and `replacement`"),
            "{}",
            error
        );
    }

    #[test]
    fn upsert_go_dependency_round_trips_replacement_shape() {
        let dir = replacement_manifest("");
        upsert_go_dependency(
            dir.path(),
            "github.com/df-mc/dragonfly",
            &GoDependency::Replaced {
                replacement_path: "github.com/fork/dragonfly".to_string(),
                replacement_version: "v1.2.0".to_string(),
                via: Some(vec!["github.com/x/y".to_string()]),
            },
        )
        .unwrap();
        let after = manifest_text(&dir);
        assert!(
            after.contains(
                r#""github.com/df-mc/dragonfly" = { replacement = "github.com/fork/dragonfly@v1.2.0", via = ["github.com/x/y"] }"#
            ),
            "{}",
            after
        );

        let reparsed = parse_manifest(dir.path()).unwrap();
        assert!(matches!(
            reparsed.go_deps()["github.com/df-mc/dragonfly"],
            GoDependency::Replaced { .. }
        ));
    }

    #[test]
    fn accepts_multi_module_monorepo_siblings() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"go.opentelemetry.io/otel" = { version = "v1.37.0", via = ["go.opentelemetry.io/contrib"] }
"go.opentelemetry.io/otel/sdk" = { version = "v1.37.0", via = ["go.opentelemetry.io/contrib"] }
"go.opentelemetry.io/otel/sdk/metric" = { version = "v1.36.0", via = ["go.opentelemetry.io/otel/sdk"] }
"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();

        assert!(check_no_subpackage_deps(&manifest).is_ok());
    }

    #[test]
    fn validate_project_name_accepts_simple_and_module_path_names() {
        assert!(validate_project_name("hello").is_ok());
        assert!(validate_project_name("github.com/enquora-net/capp-ast").is_ok());
    }

    #[test]
    fn validate_project_name_rejects_empty_elements_and_bad_chars() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name("/github.com/x").is_err());
        assert!(validate_project_name("github.com/x/").is_err());
        assert!(validate_project_name("github.com//x").is_err());
        assert!(validate_project_name("has space").is_err());
    }
}
