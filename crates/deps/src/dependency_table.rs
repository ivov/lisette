//! Parses a bare `[dependencies.go]` table, as a script writes it.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::GoDependency;

#[derive(Debug)]
pub struct DependencyTable {
    pub deps: BTreeMap<String, GoDependency>,
    pub spans: BTreeMap<String, Range<usize>>,
}

pub struct TableError {
    pub message: String,
    pub range: Option<Range<usize>>,
}

impl TableError {
    fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            range: None,
        }
    }
}

const TABLE: &str = "dependencies";
const GO: &str = "go";

pub fn parse_dependency_table(text: &str) -> Result<DependencyTable, TableError> {
    let document = toml_edit::ImDocument::parse(text).map_err(|error| TableError {
        message: error.message().to_string(),
        range: error.span(),
    })?;

    for (key, _) in document.iter() {
        if key != TABLE {
            return Err(TableError::plain(format!(
                "`[{}]` is not allowed here, only `[dependencies.go]`",
                key
            )));
        }
    }

    let Some(dependencies) = document.get(TABLE) else {
        return Ok(DependencyTable {
            deps: BTreeMap::new(),
            spans: BTreeMap::new(),
        });
    };
    for (key, _) in dependencies.as_table().into_iter().flat_map(|t| t.iter()) {
        if key != GO {
            return Err(TableError::plain(format!(
                "`[dependencies.{}]` is not allowed here, only `[dependencies.go]`",
                key
            )));
        }
    }

    let (deps, spans) = match dependencies.get(GO) {
        Some(go) => (deserialize_go_table(go)?, entry_spans(go)),
        None => (BTreeMap::new(), BTreeMap::new()),
    };

    Ok(DependencyTable { deps, spans })
}

pub(crate) fn deserialize_go_table(
    go: &toml_edit::Item,
) -> Result<BTreeMap<String, GoDependency>, TableError> {
    let mut document = toml_edit::DocumentMut::new();
    document.insert(GO, go.clone());
    Ok(toml_edit::de::from_document::<Wrapper>(document)
        .map_err(|error| TableError {
            message: error.message().to_string(),
            range: error.span(),
        })?
        .go)
}

fn entry_spans(go: &toml_edit::Item) -> BTreeMap<String, Range<usize>> {
    let Some(table) = go.as_table() else {
        return BTreeMap::new();
    };
    table
        .iter()
        .filter_map(|(name, item)| {
            let key = table.key(name)?.span()?;
            let end = item.as_value().and_then(toml_edit::Value::span);
            Some((name.to_string(), key.start..end.map_or(key.end, |v| v.end)))
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct Wrapper {
    go: BTreeMap<String, GoDependency>,
}

pub fn validate_script_entry(module_path: &str, dep: &GoDependency) -> Result<(), String> {
    crate::module_path::check_module_path(module_path)
        .map_err(|reason| format!("`{}` is not a Go module path: {}", module_path, reason))?;

    let version = match dep {
        GoDependency::Remote { version, .. } => version,
        GoDependency::Replaced { .. } => {
            return Err(format!(
                "`{}` uses `replace`, which a script cannot do. Only a project can redirect a module",
                module_path
            ));
        }
    };

    if !crate::is_exact_version(version) {
        return Err(format!(
            "`{}` is pinned to `{}`, which is not an exact version. Write a full version such as `v1.2.3`",
            module_path, version
        ));
    }
    crate::check_version_matches_path(module_path, version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> BTreeMap<String, GoDependency> {
        parse_dependency_table(text)
            .unwrap_or_else(|e| panic!("{}", e.message))
            .deps
    }

    #[test]
    fn reads_every_shape_a_manifest_holds() {
        let deps = table(
            "[dependencies.go]\n\"go.uber.org/zap\" = \"v1.28.0\"\n\"go.uber.org/multierr\" = { version = \"v1.10.0\", via = [\"go.uber.org/zap\"] }\n",
        );

        assert_eq!(deps.len(), 2);
        assert_eq!(
            deps["go.uber.org/multierr"].via(),
            Some(["go.uber.org/zap".to_string()].as_slice())
        );
    }

    #[test]
    fn an_entry_span_covers_key_and_value() {
        let text =
            "[dependencies.go]\n\"a.b/c\" = \"v1.0.0\"\n\"d.e/f\" = { version = \"v2.0.0\" }\n";
        let table = parse_dependency_table(text).ok().unwrap();

        assert_eq!(
            &text[table.spans["a.b/c"].clone()],
            "\"a.b/c\" = \"v1.0.0\""
        );
        assert_eq!(
            &text[table.spans["d.e/f"].clone()],
            "\"d.e/f\" = { version = \"v2.0.0\" }"
        );
    }

    #[test]
    fn an_empty_table_is_not_an_error() {
        assert!(table("[dependencies.go]\n").is_empty());
    }

    #[test]
    fn a_toml_error_carries_a_range() {
        let error = parse_dependency_table("[dependencies.go]\n\"x.y/z\" = \n").unwrap_err();

        assert!(error.range.is_some());
    }

    #[test]
    fn other_sections_are_rejected_by_name() {
        for text in [
            "[project]\nname = \"x\"\n",
            "[dependencies.rust]\n\"x\" = \"1\"\n",
        ] {
            let error = parse_dependency_table(text).unwrap_err();
            assert!(error.message.contains("only `[dependencies.go]`"), "{text}");
        }
    }

    fn remote(version: &str) -> GoDependency {
        GoDependency::Remote {
            version: version.to_string(),
            via: None,
        }
    }

    #[test]
    fn script_entries_must_be_exact_and_well_formed() {
        assert!(validate_script_entry("github.com/google/uuid", &remote("v1.6.0")).is_ok());
        assert!(
            validate_script_entry(
                "github.com/docker/docker",
                &remote("v20.10.24+incompatible")
            )
            .is_ok()
        );

        assert!(validate_script_entry("fmt", &remote("v1.0.0")).is_err());
        assert!(validate_script_entry("github.com//x", &remote("v1.0.0")).is_err());
        assert!(validate_script_entry("github.com/google/uuid", &remote("latest")).is_err());
        assert!(validate_script_entry("github.com/google/uuid", &remote("v1")).is_err());
        assert!(validate_script_entry("example.com/lib/v2", &remote("v1.0.0")).is_err());
    }

    #[test]
    fn a_nested_module_is_an_ordinary_entry() {
        for (module_path, version) in [
            ("go.opentelemetry.io/otel", "v1.32.0"),
            ("go.opentelemetry.io/otel/sdk/metric", "v1.32.0"),
        ] {
            assert!(validate_script_entry(module_path, &remote(version)).is_ok());
        }
    }

    #[test]
    fn a_replace_is_project_only() {
        let dep = GoDependency::Replaced {
            source: crate::ReplacementSource::Local {
                path: "../local".to_string(),
            },
            via: None,
        };

        assert!(validate_script_entry("example.com/lib", &dep).is_err());
    }
}
