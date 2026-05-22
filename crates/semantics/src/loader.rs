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

pub trait Loader {
    /// Scans a folder and returns all `.lis` files keyed by bare filename.
    fn scan_folder(&self, folder: &str) -> Files;
}
