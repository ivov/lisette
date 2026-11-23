//! The plain text lists that a test reads beside its own file: `typecheck_only.txt` names programs that only type-check and do not run, and `failing*.txt` names programs that fail today.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

/// Reads a list of names at `relative_path`, relative to the `tests` directory: one name per line, with an optional `#` note.
pub fn read_names(relative_path: &str) -> HashSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let Ok(content) = fs::read_to_string(&path) else {
        return HashSet::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let name = line.split('#').next().unwrap_or("").trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

/// A plain text list of program names that fail today, one per line, with an optional `#` note.
pub struct FailingList {
    relative_path: String,
    names: BTreeSet<String>,
}

impl FailingList {
    /// Reads the list at `relative_path`, relative to the `tests` directory.
    pub fn read(relative_path: &str) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
        assert!(path.exists(), "missing failing list {}", path.display());
        Self {
            relative_path: relative_path.to_string(),
            names: read_names(relative_path).into_iter().collect(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Fails on an unlisted failure, on a listed case that passed, and with `every_case_ran` on a listed case that did not run.
    pub fn assert_matches(
        &self,
        ran: &BTreeSet<String>,
        failures: &BTreeMap<String, String>,
        every_case_ran: bool,
    ) {
        let new: Vec<_> = failures
            .iter()
            .filter(|(name, _)| !self.names.contains(*name))
            .collect();
        let passing: Vec<_> = self
            .names
            .iter()
            .filter(|name| ran.contains(*name) && !failures.contains_key(*name))
            .collect();
        let missing: Vec<_> = if every_case_ran {
            self.names
                .iter()
                .filter(|name| !ran.contains(*name))
                .collect()
        } else {
            Vec::new()
        };
        for (name, reason) in &new {
            eprintln!("  NEW {name}: {reason}");
        }
        for name in &passing {
            eprintln!("  PASSES {name}");
        }
        for name in &missing {
            eprintln!("  NO SUCH CASE {name}");
        }
        let known_count = failures.len() - new.len();
        eprintln!(
            "{}: {} cases ran, {} failures ({known_count} known)",
            self.relative_path,
            ran.len(),
            failures.len(),
        );
        assert!(
            new.is_empty() && passing.is_empty() && missing.is_empty(),
            "tests/{list} is out of date: {} new failure(s) (fix them, or add the names to the list), {} listed case(s) now pass and {} listed case(s) do not exist (remove these names from the list). Each name is printed above.",
            new.len(),
            passing.len(),
            missing.len(),
            list = self.relative_path,
        );
    }
}
