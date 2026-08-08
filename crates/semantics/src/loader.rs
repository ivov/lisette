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

/// The predicate package discovery roots on. Shared so callers that pre-scan
/// `src/` cannot disagree with the graph about what a production package is.
pub fn is_production_package_file(name: &str) -> bool {
    name.ends_with(".lis") && !name.ends_with(".test.lis") && !is_typedef_file(name)
}

pub fn is_typedef_file(name: &str) -> bool {
    name.ends_with(".d.lis")
}

pub const EXTERNAL_TESTS_DIR: &str = "tests";

pub use syntax::ROOT_IMPORT;

pub fn is_external_test_package(package_id: &str) -> bool {
    package_id == EXTERNAL_TESTS_DIR
        || package_id
            .strip_prefix(EXTERNAL_TESTS_DIR)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub fn import_display_name(package_id: &str) -> &str {
    if package_id == crate::store::ENTRY_PACKAGE_ID {
        ROOT_IMPORT
    } else {
        package_id
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

#[derive(Debug, Clone, Copy)]
enum DiscoveredPackageContents {
    Production,
    Tests,
    ProductionAndTests,
}

impl DiscoveredPackageContents {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Production, Self::Tests) | (Self::Tests, Self::Production) => {
                Self::ProductionAndTests
            }
            (Self::ProductionAndTests, _) | (_, Self::ProductionAndTests) => {
                Self::ProductionAndTests
            }
            (Self::Production, Self::Production) | (Self::Tests, Self::Tests) => self,
        }
    }

    fn has_production(self) -> bool {
        matches!(self, Self::Production | Self::ProductionAndTests)
    }

    fn has_tests(self) -> bool {
        matches!(self, Self::Tests | Self::ProductionAndTests)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredPackages {
    packages: HashMap<String, DiscoveredPackageContents>,
}

impl DiscoveredPackages {
    pub fn add_production(&mut self, package_id: String, has_internal_tests: bool) {
        let contents = if has_internal_tests {
            DiscoveredPackageContents::ProductionAndTests
        } else {
            DiscoveredPackageContents::Production
        };
        self.add(package_id, contents);
    }

    pub fn add_test_root(&mut self, package_id: String) {
        self.add(package_id, DiscoveredPackageContents::Tests);
    }

    fn add(&mut self, package_id: String, incoming: DiscoveredPackageContents) {
        self.packages
            .entry(package_id)
            .and_modify(|existing| *existing = existing.merge(incoming))
            .or_insert(incoming);
    }

    pub fn production_packages(&self) -> impl Iterator<Item = &String> {
        self.packages
            .iter()
            .filter_map(|(package_id, kind)| kind.has_production().then_some(package_id))
    }

    pub fn internal_test_roots(&self) -> impl Iterator<Item = &String> {
        self.packages.iter().filter_map(|(package_id, kind)| {
            (kind.has_tests() && !is_external_test_package(package_id)).then_some(package_id)
        })
    }

    pub fn external_test_roots(&self) -> impl Iterator<Item = &String> {
        self.packages.iter().filter_map(|(package_id, kind)| {
            (kind.has_tests() && is_external_test_package(package_id)).then_some(package_id)
        })
    }

    pub fn test_roots(&self) -> impl Iterator<Item = &String> {
        self.packages
            .iter()
            .filter_map(|(package_id, kind)| kind.has_tests().then_some(package_id))
    }
}

pub trait Loader: Sync {
    /// Scans a folder and returns all `.lis` files keyed by bare filename.
    fn scan_folder(&self, folder: &str) -> Files;

    fn discover_packages(&self) -> DiscoveredPackages {
        DiscoveredPackages::default()
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

    fn discover_packages(&self) -> DiscoveredPackages {
        let mut discovered = DiscoveredPackages::default();
        for (package_id, files) in &self.folders {
            let has_tests = files.keys().any(|name| name.ends_with(".test.lis"));
            if is_external_test_package(package_id) {
                if has_tests {
                    discovered.add_test_root(package_id.clone());
                }
                continue;
            }

            let has_production = files.keys().any(|name| is_production_package_file(name));
            let is_internal_test_root =
                has_tests && files.keys().any(|name| counts_for_internal_test_root(name));
            if has_production {
                discovered.add_production(package_id.clone(), is_internal_test_root);
            } else if is_internal_test_root {
                discovered.add_test_root(package_id.clone());
            }
        }
        discovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_test_namespace_matches_exact_prefix_only() {
        assert!(is_external_test_package("tests"));
        assert!(is_external_test_package("tests/integration"));
        assert!(!is_external_test_package("testsuite"));
        assert!(!is_external_test_package("tests_helper"));
        assert!(!is_external_test_package("math/tests"));
    }

    #[test]
    fn memory_loader_classifies_external_test_folders() {
        let mut loader = MemoryLoader::new();
        loader.add_file("math", "math.lis", "pub fn add() {}");
        loader.add_file("tests", "arithmetic.test.lis", "#[test]\nfn t() {}");
        loader.add_file("tests/flows", "flow.test.lis", "#[test]\nfn t() {}");

        let discovered = loader.discover_packages();
        assert_eq!(
            discovered
                .production_packages()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["math".to_string()]
        );
        assert_eq!(discovered.internal_test_roots().count(), 0);
        let mut external = discovered
            .external_test_roots()
            .cloned()
            .collect::<Vec<_>>();
        external.sort();
        assert_eq!(
            external,
            vec!["tests".to_string(), "tests/flows".to_string()]
        );
    }

    #[test]
    fn package_contents_merge_without_a_kind_transition_protocol() {
        let mut discovered = DiscoveredPackages::default();
        discovered.add_test_root("math".into());
        discovered.add_production("math".into(), false);

        assert_eq!(discovered.production_packages().count(), 1);
        assert_eq!(discovered.internal_test_roots().count(), 1);
        assert_eq!(discovered.test_roots().count(), 1);
    }

    #[test]
    fn external_test_identity_comes_from_the_package_path() {
        let mut discovered = DiscoveredPackages::default();
        discovered.add_test_root("tests/integration".into());

        assert_eq!(discovered.internal_test_roots().count(), 0);
        assert_eq!(discovered.external_test_roots().count(), 1);
    }
}
