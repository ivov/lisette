use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;

use super::definition::{Definition, Visibility};
use super::file::{File, FileImport};

pub type ModuleId = String;

#[derive(Debug, Clone)]
pub struct Module {
    pub id: String,
    /// file ID -> .lis file
    pub files: HashMap<u32, File>,
    /// file ID -> .d.lis file (declarations only)
    pub typedefs: HashMap<u32, File>,
    /// qualified name -> definition
    pub definitions: HashMap<EcoString, Definition>,
}

impl Module {
    pub fn new(id: &str) -> Module {
        Module {
            id: id.to_string(),
            files: Default::default(),
            typedefs: Default::default(),
            definitions: Default::default(),
        }
    }

    pub fn nominal() -> Module {
        Module::new("**nominal")
    }

    pub fn is_public(&self, qualified_name: &str) -> bool {
        if let Some(definition) = self.definitions.get(qualified_name) {
            return definition.visibility() == &Visibility::Public;
        }

        false
    }

    pub fn get_file(&self, file_id: u32) -> Option<&File> {
        self.files.get(&file_id)
    }

    pub fn file_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.files.keys().copied()
    }

    pub fn get_typedef_by_id(&self, file_id: u32) -> Option<&File> {
        self.typedefs.get(&file_id)
    }

    pub fn get_typedef_by_id_mut(&mut self, file_id: u32) -> Option<&mut File> {
        self.typedefs.get_mut(&file_id)
    }

    pub fn typedef_imports(&self) -> Vec<FileImport> {
        self.typedefs.values().flat_map(|f| f.imports()).collect()
    }

    pub fn all_typedefs(&self) -> impl Iterator<Item = &File> {
        self.typedefs.values()
    }

    pub fn is_internal(&self) -> bool {
        self.id == "prelude" || self.id == "**nominal" || self.id.starts_with("go:")
    }

    pub fn is_empty_stub(&self) -> bool {
        self.files.is_empty() && self.typedefs.is_empty() && self.definitions.is_empty()
    }
}

pub struct ModuleInfo {
    pub id: String,
    pub path: String,
    pub file_ids: Vec<u32>,
    pub typedef_ids: Vec<u32>,
}
