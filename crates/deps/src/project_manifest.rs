use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub project: Project,
    toolchain: Option<Toolchain>,
    dependencies: Option<Dependencies>,
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
        source: ReplacementSource,
        via: Option<Vec<String>>,
    },
}

/// The right-hand side of a Go `replace` directive.
#[derive(Debug, Clone)]
pub enum ReplacementSource {
    Module {
        path: String,
        version: String,
    },
    /// Relative to `lisette.toml` unless absolute.
    Local {
        path: String,
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
            GoDependency::Replaced { source, .. } => GoDependency::Replaced {
                source: source.clone(),
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
                    "a version string, or a table with exactly one of `version`, `replacement`, or `path` (and optional `via`)",
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
                let mut path: Option<String> = None;
                let mut via: Option<Vec<String>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "version" => version = Some(map.next_value()?),
                        "replacement" => replacement = Some(map.next_value()?),
                        "path" => path = Some(map.next_value()?),
                        "via" => via = Some(map.next_value()?),
                        other => {
                            return Err(de::Error::unknown_field(
                                other,
                                &["version", "replacement", "path", "via"],
                            ));
                        }
                    }
                }

                match (version, replacement, path) {
                    (Some(version), None, None) => Ok(GoDependency::Remote { version, via }),
                    (None, Some(replacement), None) => {
                        let (path, version) =
                            split_replacement(&replacement).map_err(de::Error::custom)?;
                        Ok(GoDependency::Replaced {
                            source: ReplacementSource::Module { path, version },
                            via,
                        })
                    }
                    (None, None, Some(path)) => Ok(GoDependency::Replaced {
                        source: ReplacementSource::Local { path },
                        via,
                    }),
                    (None, None, None) => Err(de::Error::custom(
                        "a Go dependency table needs `version`, `replacement`, or `path`",
                    )),
                    _ => Err(de::Error::custom(
                        "a Go dependency sets exactly one of `version`, `replacement`, or `path`",
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

/// Walks upward from `start`, a file or a directory, for the nearest project root.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if current.join("lisette.toml").is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn parse_manifest(project_root: &Path) -> Result<Manifest, String> {
    let project_toml_path = project_root.join("lisette.toml");

    let bytes = fs::read(&project_toml_path)
        .map_err(|_| format!("No `lisette.toml` manifest in `{}`", project_root.display()))?;
    let content =
        strip_bom_to_str(&bytes).map_err(|e| format!("Invalid `lisette.toml` manifest: {}", e))?;

    let manifest: Manifest = toml_edit::de::from_str(content)
        .map_err(|e| format!("Invalid `lisette.toml` manifest: {}", e))?;
    validate_go_dep_paths(&manifest)?;
    Ok(manifest)
}

/// A dotless path is reported first and by name, since it reads as the Go
/// standard library rather than as a malformed path.
fn validate_go_dep_paths(manifest: &Manifest) -> Result<(), String> {
    for (key, dep) in &manifest.go_deps() {
        if let GoDependency::Replaced { source, .. } = dep {
            if !crate::is_third_party(key) {
                return Err(match source {
                    ReplacementSource::Module { .. } => format!(
                        "`{}` in `[dependencies.go]` has a `replace` but is not a third-party module path (its first path segment needs a dot)",
                        key
                    ),
                    ReplacementSource::Local { .. } => format!(
                        "`{}` in `[dependencies.go]` has a local `path` but no dot in its first path segment; lisette reads dotless paths as Go standard library packages, so a local module needs a dotted module path like `example.com/{}` (a lisette limitation, not a Go rule)",
                        key, key
                    ),
                });
            }
            if let ReplacementSource::Module { path, .. } = source
                && !crate::is_third_party(path)
            {
                return Err(format!(
                    "the `replace` target `{}` for `{}` is not a third-party module path",
                    path, key
                ));
            }
        }

        crate::module_path::check_module_path(key).map_err(|reason| {
            format!(
                "`{}` in `[dependencies.go]` is not a Go module path: {}",
                key, reason
            )
        })?;

        // A local `path` names a directory, not a module.
        if let GoDependency::Replaced {
            source: ReplacementSource::Module { path, .. },
            ..
        } = dep
        {
            crate::module_path::check_module_path(path).map_err(|reason| {
                format!(
                    "the `replace` target `{}` for `{}` is not a Go module path: {}",
                    path, key, reason
                )
            })?;
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
    // A `Local` entry's own `go.mod` establishes a module boundary, so a
    // nested key is a distinct module, not a subpackage.
    let is_local = |d: &GoDependency| {
        matches!(
            d,
            GoDependency::Replaced {
                source: ReplacementSource::Local { .. },
                ..
            }
        )
    };

    for (key, dep) in &deps {
        let Some((parent, parent_dep)) = deps
            .iter()
            .find(|(other, _)| other.as_str() != key.as_str() && is_pkg_under(key, other))
        else {
            continue;
        };

        if has_via(dep) || has_via(parent_dep) || is_local(dep) || is_local(parent_dep) {
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

pub struct ManifestDocument {
    path: PathBuf,
    encoding: ManifestEncoding,
    pub document: toml_edit::DocumentMut,
}

impl ManifestDocument {
    pub fn open(project_root: &Path) -> Result<Self, String> {
        let path = project_root.join("lisette.toml");
        let (encoding, document) = open_manifest(&path)?;
        Ok(Self {
            path,
            encoding,
            document,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        save_manifest(&self.path, &self.encoding, &self.document)
    }
}

pub fn go_deps_of_document(
    document: &toml_edit::DocumentMut,
) -> Result<BTreeMap<String, GoDependency>, String> {
    let Some(go) = document
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("go"))
    else {
        return Ok(BTreeMap::new());
    };

    let mut wrapped = toml_edit::DocumentMut::new();
    wrapped.insert("go", go.clone());
    toml_edit::de::from_document::<GoTable>(wrapped)
        .map(|table| table.go)
        .map_err(|error| format!("Invalid `[dependencies.go]`: {}", error.message()))
}

#[derive(Deserialize)]
struct GoTable {
    go: BTreeMap<String, GoDependency>,
}

/// Add or update a Go dependency in `lisette.toml`, written in the shape matching its variant.
pub fn upsert_go_dependency(
    project_root: &Path,
    module_path: &str,
    dep: &GoDependency,
) -> Result<(), String> {
    let mut manifest = ManifestDocument::open(project_root)?;
    upsert_into_document(&mut manifest.document, module_path, dep)?;
    manifest.save()
}

pub fn upsert_into_document(
    manifest: &mut toml_edit::DocumentMut,
    module_path: &str,
    dep: &GoDependency,
) -> Result<(), String> {
    let go = ensure_go_deps_table(manifest)?;

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
        GoDependency::Replaced { source, .. } => {
            let mut inline = toml_edit::InlineTable::new();
            match source {
                ReplacementSource::Module { path, version } => {
                    inline.insert(
                        "replacement",
                        format!("{}@{}", path, version).as_str().into(),
                    );
                }
                ReplacementSource::Local { path } => {
                    inline.insert("path", path.as_str().into());
                }
            }
            if let Some(via_list) = via {
                inline.insert("via", via_array(&via_list));
            }
            go.insert(
                module_path,
                toml_edit::value(toml_edit::Value::InlineTable(inline)),
            );
        }
    }

    Ok(())
}

fn via_array(via_list: &[String]) -> toml_edit::Value {
    let mut arr = toml_edit::Array::new();
    for v in via_list {
        arr.push(v.as_str());
    }
    toml_edit::Value::Array(arr)
}

pub fn remove_from_document(manifest: &mut toml_edit::DocumentMut, module_path: &str) {
    if let Some(deps) = manifest
        .get_mut("dependencies")
        .and_then(|d| d.as_table_mut())
        && let Some(go) = deps.get_mut("go").and_then(|g| g.as_table_mut())
    {
        go.remove(module_path);
    }
}

/// Trimmed transitive dep. `removed_parents` are parents dropped from `via`.
pub struct TrimmedVia {
    pub module_path: String,
    pub removed_parents: Vec<String>,
}

struct ResolveReport {
    promoted: Vec<String>,
    removed: Vec<String>,
}

/// Drop `via` parents that are no longer declared. Never deletes entries.
/// `resolve_empty_via_in` handles entries left with `via = []`.
fn trim_dead_via_in(
    document: &mut toml_edit::DocumentMut,
    live_deps: &BTreeMap<String, GoDependency>,
) -> Result<Vec<TrimmedVia>, String> {
    let live_paths: HashSet<&str> = live_deps.keys().map(|s| s.as_str()).collect();

    let mut trimmed = Vec::new();

    for (dep_path, dep) in live_deps {
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

        upsert_into_document(document, dep_path, &dep.with_via(Some(canonical)))?;
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
/// Each import maps to a single best key: its longest declared prefix. E.g.
/// `k8s.io/api/core/v1` maps to `k8s.io/api` (not `k8s.io`) when both are
/// declared. The key is already declared, so this asks only whether an import
/// reaches it, never which module owns a package.
fn resolve_empty_via_in(
    document: &mut toml_edit::DocumentMut,
    live_deps: &BTreeMap<String, GoDependency>,
    imported_pkgs: &[String],
) -> Result<ResolveReport, String> {
    let mut matched: HashSet<String> = HashSet::new();
    for pkg in imported_pkgs {
        if let Some((module, _)) = find_module_for_pkg(live_deps, pkg) {
            matched.insert(module.to_string());
        }
    }

    let mut promoted = Vec::new();
    let mut removed = Vec::new();

    for (dep_path, dep) in live_deps {
        let Some(via) = dep.via() else { continue };
        if !via.is_empty() {
            continue;
        }

        if matched.contains(dep_path.as_str()) {
            upsert_into_document(document, dep_path, &dep.with_via(None))?;
            promoted.push(dep_path.clone());
        } else {
            remove_from_document(document, dep_path);
            removed.push(dep_path.clone());
        }
    }

    Ok(ResolveReport { promoted, removed })
}

#[derive(Default)]
pub struct ViaChanges {
    pub trimmed: Vec<TrimmedVia>,
    pub promoted: Vec<String>,
    pub removed: Vec<String>,
}

impl ViaChanges {
    pub fn is_empty(&self) -> bool {
        self.trimmed.is_empty() && self.promoted.is_empty() && self.removed.is_empty()
    }
}

pub fn finalize_via(
    document: &mut toml_edit::DocumentMut,
    imported_pkgs: &[String],
) -> Result<ViaChanges, String> {
    let mut changes = ViaChanges::default();

    loop {
        let trimmed = trim_dead_via_in(document, &go_deps_of_document(document)?)?;
        let report =
            resolve_empty_via_in(document, &go_deps_of_document(document)?, imported_pkgs)?;

        let changed =
            !trimmed.is_empty() || !report.promoted.is_empty() || !report.removed.is_empty();
        changes.trimmed.extend(trimmed);
        changes.promoted.extend(report.promoted);
        changes.removed.extend(report.removed);

        if !changed {
            return Ok(changes);
        }
    }
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

    fn finalize(dir: &TempDir, imported: &[&str]) -> ViaChanges {
        let imported: Vec<String> = imported.iter().map(|p| (*p).to_string()).collect();
        let mut manifest = ManifestDocument::open(dir.path()).unwrap();
        let changes = finalize_via(&mut manifest.document, &imported).unwrap();
        if !changes.is_empty() {
            manifest.save().unwrap();
        }
        changes
    }

    #[test]
    fn project_root_is_found_from_a_file_a_directory_or_not_at_all() {
        let dir = project_with("[project]\nname = \"demo\"\nversion = \"0.1.0\"\n");
        let root = dir.path().canonicalize().unwrap();
        let nested = root.join("src/util");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("util.lis");
        std::fs::write(&file, "pub fn f() {}\n").unwrap();

        assert_eq!(find_project_root(&file).as_deref(), Some(root.as_path()));
        assert_eq!(find_project_root(&nested).as_deref(), Some(root.as_path()));
        assert_eq!(find_project_root(&root).as_deref(), Some(root.as_path()));

        let outside = tempfile::tempdir().unwrap();
        let orphan = outside.path().join("loose.lis");
        std::fs::write(&orphan, "fn main() {}\n").unwrap();
        assert_eq!(find_project_root(&orphan), None);
    }

    #[test]
    fn a_malformed_table_is_an_error_not_an_empty_one() {
        let document: toml_edit::DocumentMut = "[dependencies.go]\n\"a.b/c\" = { nonsense = 1 }\n"
            .parse()
            .unwrap();

        assert!(go_deps_of_document(&document).is_err());
    }

    #[test]
    fn a_key_go_would_refuse_is_not_a_dependency() {
        for key in [
            "fmt",
            "github.com//x",
            "GitHub.com/x/y",
            "example.com/lib/v1",
        ] {
            let dir = project_with(&format!(
                "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n\"{}\" = \"v1.0.0\"\n",
                key
            ));
            let error = parse_manifest(dir.path()).unwrap_err();
            assert!(
                error.contains("not a Go module path"),
                "{key} gave: {error}"
            );
        }

        let dir = project_with(
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n\"gopkg.in/yaml.v3\" = \"v3.0.1\"\n\"example.com/lib/v2\" = \"v2.0.0\"\n",
        );
        assert!(parse_manifest(dir.path()).is_ok());
    }

    #[test]
    fn a_replacement_is_held_to_the_same_module_path_rule() {
        for entry in [
            "\"github.com//x\" = { replacement = \"example.com/y@v1.0.0\" }",
            "\"example.com/lib/v1\" = { replacement = \"example.com/y@v1.0.0\" }",
            "\"example.com/lib\" = { replacement = \"github.com//y@v1.0.0\" }",
            "\"example.com/lib\" = { replacement = \"example.com/y/v1@v1.0.0\" }",
            "\"github.com//x\" = { path = \"../local\" }",
        ] {
            let dir = project_with(&format!(
                "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n{}\n",
                entry
            ));
            let error = parse_manifest(dir.path()).unwrap_err();
            assert!(
                error.contains("not a Go module path"),
                "{entry} gave: {error}"
            );
        }

        let dir = project_with(
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies.go]\n\"example.com/lib\" = { replacement = \"github.com/you/lib@v1.0.0\" }\n\"example.com/other\" = { path = \"../local\" }\n",
        );
        assert!(parse_manifest(dir.path()).is_ok());
    }

    #[test]
    fn a_directory_named_like_the_manifest_is_not_a_project_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("lisette.toml")).unwrap();

        assert_eq!(find_project_root(dir.path()), None);
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

        let changes = finalize(&dir, &["github.com/gorilla/context"]);

        assert_eq!(changes.promoted, vec!["github.com/gorilla/context"]);
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

        let changes = finalize(&dir, &[]);

        assert_eq!(changes.removed, vec!["github.com/gorilla/context"]);
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

        finalize(&dir, &[]);

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

        let changes = finalize(&dir, &["k8s.io/api/core/v1"]);

        assert_eq!(changes.promoted, vec!["k8s.io/api"]);
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

        let changes = finalize(&dir, &["github.com/gorilla/mux"]);

        assert!(changes.is_empty());
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
                source: ReplacementSource::Module { path, version },
                via,
            } => {
                assert_eq!(path, "github.com/fork/dragonfly");
                assert_eq!(version, "v1.2.0");
                assert!(via.is_none());
            }
            other => panic!("expected Replaced module, got {:?}", other),
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
                source: ReplacementSource::Module { version, .. },
                via,
            } => {
                assert_eq!(version, "v0.0.0-20260101000000-abcdef123456");
                assert_eq!(
                    via.as_deref(),
                    Some(["github.com/x/y".to_string()].as_slice())
                );
            }
            other => panic!("expected Replaced module, got {:?}", other),
        }
    }

    #[test]
    fn parses_local_path_entry() {
        let dir = replacement_manifest(r#""example.com/me/foo" = { path = "../foo" }"#);
        let manifest = parse_manifest(dir.path()).unwrap();
        match &manifest.go_deps()["example.com/me/foo"] {
            GoDependency::Replaced {
                source: ReplacementSource::Local { path },
                via,
            } => {
                assert_eq!(path, "../foo");
                assert!(via.is_none());
            }
            other => panic!("expected Replaced local, got {:?}", other),
        }
    }

    #[test]
    fn parses_local_path_entry_with_via() {
        let dir = replacement_manifest(
            r#""example.com/me/child" = { path = "../child", via = ["example.com/me/foo"] }"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();
        let deps = manifest.go_deps();
        assert_eq!(
            deps["example.com/me/child"].via().unwrap(),
            ["example.com/me/foo".to_string()].as_slice()
        );
    }

    #[test]
    fn rejects_dotless_local_key_as_lisette_limitation() {
        let dir = replacement_manifest(r#""foo" = { path = "../foo" }"#);
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(error.contains("standard library"), "{}", error);
        assert!(
            error.contains("lisette limitation, not a Go rule"),
            "{}",
            error
        );
    }

    #[test]
    fn rejects_path_combined_with_version_or_replacement() {
        let dir = replacement_manifest(
            r#""example.com/me/foo" = { path = "../foo", version = "v1.0.0" }"#,
        );
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(error.contains("exactly one"), "{}", error);

        let dir = replacement_manifest(
            r#""example.com/me/foo" = { path = "../foo", replacement = "github.com/fork/foo@v1.0.0" }"#,
        );
        let error = parse_manifest(dir.path()).unwrap_err();
        assert!(error.contains("exactly one"), "{}", error);
    }

    #[test]
    fn upsert_go_dependency_round_trips_local_shape() {
        let dir = replacement_manifest("");
        upsert_go_dependency(
            dir.path(),
            "example.com/me/foo",
            &GoDependency::Replaced {
                source: ReplacementSource::Local {
                    path: "../foo".to_string(),
                },
                via: None,
            },
        )
        .unwrap();
        let after = manifest_text(&dir);
        assert!(
            after.contains(r#""example.com/me/foo" = { path = "../foo" }"#),
            "{}",
            after
        );

        let reparsed = parse_manifest(dir.path()).unwrap();
        assert!(matches!(
            reparsed.go_deps()["example.com/me/foo"],
            GoDependency::Replaced {
                source: ReplacementSource::Local { .. },
                ..
            }
        ));
    }

    #[test]
    fn nested_local_modules_are_not_subpackages() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"example.com/acme/parent" = { path = "../parent" }
"example.com/acme/parent/child" = { path = "../parent/child" }
"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();
        assert!(check_no_subpackage_deps(&manifest).is_ok());
    }

    #[test]
    fn local_module_under_remote_pin_is_not_a_subpackage() {
        let dir = project_with(
            r#"[project]
name = "demo"
version = "0.1.0"

[dependencies.go]
"example.com/acme/parent" = "v1.0.0"
"example.com/acme/parent/child" = { path = "../child" }
"#,
        );
        let manifest = parse_manifest(dir.path()).unwrap();
        assert!(check_no_subpackage_deps(&manifest).is_ok());
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
            error.contains("exactly one of `version`, `replacement`, or `path`"),
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
                source: ReplacementSource::Module {
                    path: "github.com/fork/dragonfly".to_string(),
                    version: "v1.2.0".to_string(),
                },
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
