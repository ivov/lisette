use rustc_hash::FxHashMap as HashMap;

/// Source content plus a cwd-relative display path for diagnostics.
/// `display_path` matches `name` for loaders that have no notion of cwd
/// (test/overlay loaders); the CLI's filesystem loader sets it to the path
/// relative to the process cwd.
#[derive(Debug, Clone)]
pub struct FileContent {
    pub source: String,
    pub display_path: String,
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

fn counts_for_test_root(name: &str) -> bool {
    name.ends_with(".lis") && !name.ends_with(".test.lis")
}

fn is_production_module_file(name: &str) -> bool {
    name.ends_with(".lis") && !name.ends_with(".test.lis") && !name.ends_with(".d.lis")
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredModules {
    pub production_modules: Vec<String>,
    pub test_roots: Vec<String>,
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
            .filter(|(_, files)| files.keys().any(|name| is_production_module_file(name)))
            .map(|(folder, _)| folder.clone())
            .collect();
        let test_roots = self
            .folders
            .iter()
            .filter(|(_, files)| {
                files.keys().any(|name| name.ends_with(".test.lis"))
                    && files.keys().any(|name| counts_for_test_root(name))
            })
            .map(|(folder, _)| folder.clone())
            .collect();
        DiscoveredModules {
            production_modules,
            test_roots,
        }
    }
}
