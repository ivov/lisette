use rustc_hash::FxHashMap as HashMap;

use super::definition::{Definition, Visibility};
use super::file::{File, FileImport};
use crate::types::Symbol;

pub type ModuleId = String;

#[derive(Debug, Clone)]
pub struct Module {
    pub id: String,
    /// file ID -> file. Source and declaration files are classified by [`File::is_d_lis`].
    pub files: HashMap<u32, File>,
    /// qualified name -> definition
    pub definitions: HashMap<Symbol, Definition>,
}

impl Module {
    pub fn new(id: &str) -> Module {
        Module {
            id: id.to_string(),
            files: Default::default(),
            definitions: Default::default(),
        }
    }

    pub fn nominal() -> Module {
        Module::new("**nominal")
    }

    pub fn is_public(&self, qualified_name: &str) -> bool {
        if let Some(definition) = self.definitions.get(qualified_name) {
            return definition.visibility == Visibility::Public;
        }

        false
    }

    pub fn get_file(&self, file_id: u32) -> Option<&File> {
        self.files.get(&file_id)
    }

    pub fn file_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.source_file_entries().map(|(file_id, _)| *file_id)
    }

    pub fn source_files(&self) -> impl Iterator<Item = &File> {
        self.files.values().filter(|file| !file.is_d_lis())
    }

    pub fn source_file_entries(&self) -> impl Iterator<Item = (&u32, &File)> {
        self.files.iter().filter(|(_, file)| !file.is_d_lis())
    }

    pub fn typedef_files(&self) -> impl Iterator<Item = &File> {
        self.files.values().filter(|file| file.is_d_lis())
    }

    pub fn is_typedef(&self, file_id: u32) -> bool {
        self.files.get(&file_id).is_some_and(File::is_d_lis)
    }

    pub fn all_imports(&self) -> Vec<FileImport> {
        self.files.values().flat_map(|f| f.imports()).collect()
    }

    pub fn is_internal(&self) -> bool {
        self.id == "prelude"
            || self.id == "**test_prelude"
            || self.id == "**nominal"
            || self.id.starts_with("go:")
    }

    pub fn is_empty_stub(&self) -> bool {
        self.files.is_empty() && self.definitions.is_empty()
    }
}
