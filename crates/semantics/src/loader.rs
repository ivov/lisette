use rustc_hash::FxHashMap as HashMap;

pub type Files = HashMap<String, String>; // filename -> content

pub trait Loader {
    /// Scans a folder and returns all `.lis` files as a map of filename to content
    fn scan_folder(&self, folder: &str) -> Files;
}
