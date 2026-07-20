use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::report::{TestRow, failed_keys};

#[derive(Serialize, Deserialize, Default)]
struct LastFailures {
    failures: Vec<FailedTest>,
}

#[derive(Serialize, Deserialize)]
struct FailedTest {
    package: String,
    test: String,
}

fn last_failures_path(target_dir: &Path) -> PathBuf {
    target_dir.join(".lisette").join("last-failures.json")
}

pub fn save(target_dir: &Path, rows: &[TestRow]) {
    let failures = failed_keys(rows)
        .into_iter()
        .map(|(package, test)| FailedTest { package, test })
        .collect();
    let path = last_failures_path(target_dir);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(&LastFailures { failures }) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load(target_dir: &Path) -> HashSet<(String, String)> {
    let Ok(json) = std::fs::read_to_string(last_failures_path(target_dir)) else {
        return HashSet::new();
    };
    serde_json::from_str::<LastFailures>(&json)
        .map(|state| {
            state
                .failures
                .into_iter()
                .map(|f| (f.package, f.test))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test::report::Status;
    use syntax::ast::Span;

    fn row(package: &str, go_name: &str, status: Status) -> TestRow {
        TestRow {
            package: package.into(),
            go_name: go_name.into(),
            name: go_name.into(),
            description: None,
            status,
            elapsed: None,
            output: String::new(),
            failure: None,
            skip_reason: None,
            logs: vec![],
            children: vec![],
            span: Span::new(0, 0, 0),
        }
    }

    #[test]
    fn save_then_load_keeps_only_failures() {
        let dir = tempfile::tempdir().unwrap();
        let rows = vec![
            row("pkg", "TestA", Status::Passed),
            row("pkg", "TestB", Status::Failed),
        ];
        save(dir.path(), &rows);

        let loaded = load(dir.path());
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains(&("pkg".to_string(), "TestB".to_string())));
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_empty());
    }
}
