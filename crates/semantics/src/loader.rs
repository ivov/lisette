use rustc_hash::FxHashMap as HashMap;

/// Source content plus a cwd-relative display path for diagnostics.
/// `display_path` matches `name` for loaders that have no notion of cwd
/// (test/overlay loaders); the CLI's filesystem loader sets it to the path
/// relative to the process cwd.
#[derive(Debug, Clone)]
pub struct FileContent {
    pub source: String,
    pub(crate) display_path: String,
}

impl FileContent {
    pub fn new(source: impl Into<String>, display_path: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            display_path: display_path.into(),
        }
    }
}

pub type Files = HashMap<String, FileContent>;

fn counts_for_internal_test_root(name: &str) -> bool {
    name.ends_with(".lis") && !name.ends_with(".test.lis")
}

/// The predicate module discovery roots on. Shared so callers that pre-scan
/// `src/` cannot disagree with the graph about what a production module is.
pub fn is_production_module_file(name: &str) -> bool {
    name.ends_with(".lis") && !name.ends_with(".test.lis") && !name.ends_with(".d.lis")
}

pub const EXTERNAL_TESTS_DIR: &str = "tests";

pub use syntax::ROOT_IMPORT;

pub fn is_external_test_module(module_id: &str) -> bool {
    module_id == EXTERNAL_TESTS_DIR
        || module_id
            .strip_prefix(EXTERNAL_TESTS_DIR)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub fn import_display_name(module_id: &str) -> &str {
    if module_id == crate::store::ENTRY_MODULE_ID {
        ROOT_IMPORT
    } else {
        module_id
    }
}

pub enum ExternalTestFileIssue {
    WrongSuffix,
    NotATestFile,
}

pub fn external_test_file_issue(name: &str) -> Option<ExternalTestFileIssue> {
    if name.ends_with(".test.lis") {
        None
    } else if name.ends_with("_test.lis") {
        Some(ExternalTestFileIssue::WrongSuffix)
    } else {
        Some(ExternalTestFileIssue::NotATestFile)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredModules {
    pub production_modules: Vec<String>,
    pub internal_test_roots: Vec<String>,
    pub external_test_roots: Vec<String>,
}

pub trait Loader: Sync {
    /// Scans a folder and returns all `.lis` files keyed by bare filename.
    fn scan_folder(&self, folder: &str) -> Files;

    fn discover_modules(&self) -> DiscoveredModules {
        DiscoveredModules::default()
    }
}

/// In-memory `Loader` keyed by folder. Use for tests, benches, the wasm
/// playground, and anywhere the source content does not live on disk.
#[derive(Debug, Clone, Default)]
pub struct MemoryLoader {
    folders: HashMap<String, Files>,
}

impl MemoryLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a file; the diagnostic display path is set to `filename`.
    pub fn add_file(&mut self, folder: &str, filename: &str, content: &str) {
        self.add_file_with_display(folder, filename, filename, content);
    }

    /// Insert a file with an explicit diagnostic display path.
    pub fn add_file_with_display(
        &mut self,
        folder: &str,
        filename: &str,
        display_path: &str,
        content: &str,
    ) {
        self.folders.entry(folder.to_string()).or_default().insert(
            filename.to_string(),
            FileContent::new(content.to_string(), display_path.to_string()),
        );
    }

    /// All registered folder names.
    pub fn folders(&self) -> Vec<String> {
        self.folders.keys().cloned().collect()
    }
}

impl Loader for MemoryLoader {
    fn scan_folder(&self, folder: &str) -> Files {
        self.folders.get(folder).cloned().unwrap_or_default()
    }

    fn discover_modules(&self) -> DiscoveredModules {
        let production_modules = self
            .folders
            .iter()
            .filter(|(folder, _)| !is_external_test_module(folder))
            .filter(|(_, files)| files.keys().any(|name| is_production_module_file(name)))
            .map(|(folder, _)| folder.clone())
            .collect();
        let internal_test_roots = self
            .folders
            .iter()
            .filter(|(folder, _)| !is_external_test_module(folder))
            .filter(|(_, files)| {
                files.keys().any(|name| name.ends_with(".test.lis"))
                    && files.keys().any(|name| counts_for_internal_test_root(name))
            })
            .map(|(folder, _)| folder.clone())
            .collect();
        let external_test_roots = self
            .folders
            .iter()
            .filter(|(folder, _)| is_external_test_module(folder))
            .filter(|(_, files)| files.keys().any(|name| name.ends_with(".test.lis")))
            .map(|(folder, _)| folder.clone())
            .collect();
        DiscoveredModules {
            production_modules,
            internal_test_roots,
            external_test_roots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_test_namespace_matches_exact_prefix_only() {
        assert!(is_external_test_module("tests"));
        assert!(is_external_test_module("tests/integration"));
        assert!(!is_external_test_module("testsuite"));
        assert!(!is_external_test_module("tests_helper"));
        assert!(!is_external_test_module("math/tests"));
    }

    #[test]
    fn memory_loader_classifies_external_test_folders() {
        let mut loader = MemoryLoader::new();
        loader.add_file("math", "math.lis", "pub fn add() {}");
        loader.add_file("tests", "arithmetic.test.lis", "#[test]\nfn t() {}");
        loader.add_file("tests/flows", "flow.test.lis", "#[test]\nfn t() {}");

        let discovered = loader.discover_modules();
        assert_eq!(discovered.production_modules, vec!["math".to_string()]);
        assert!(discovered.internal_test_roots.is_empty());
        let mut external = discovered.external_test_roots;
        external.sort();
        assert_eq!(
            external,
            vec!["tests".to_string(), "tests/flows".to_string()]
        );
    }
}
