use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

use super::file::File;
use super::{Definition, Visibility};
use crate::types::Symbol;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId(String);

impl PackageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PackageId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for PackageId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Borrow<str> for PackageId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for PackageId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for PackageId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for PackageId {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for PackageId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

pub fn is_internal_package_id(id: &str) -> bool {
    id == "prelude" || id == "**test_prelude" || id == "**nominal" || id.starts_with("go:")
}

#[derive(Debug, Clone)]
pub struct Package {
    pub id: String,
    /// file ID -> file. Source and declaration files are classified by [`File::is_d_lis`].
    pub files: HashMap<u32, File>,
    /// qualified name -> definition
    pub definitions: HashMap<Symbol, Definition>,
    /// Set when an import cycle keeps the package out of inference: nothing
    /// registers its files, so its exports are read from syntax.
    pub uninferred_exports: Option<UninferredExports>,
}

#[derive(Debug, Clone)]
pub enum UninferredExports {
    /// Read from the syntax of the package's files.
    Known(HashSet<EcoString>),
    /// A file did not parse, so its declarations cannot be read.
    Unreadable,
}

impl UninferredExports {
    pub fn may_contain(&self, member: &str) -> bool {
        match self {
            Self::Known(names) => names.contains(member),
            Self::Unreadable => true,
        }
    }
}

impl Package {
    pub fn new(id: &str) -> Package {
        Package {
            id: id.to_string(),
            files: Default::default(),
            definitions: Default::default(),
            uninferred_exports: None,
        }
    }

    pub fn nominal() -> Package {
        Package::new("**nominal")
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

    pub fn is_empty_stub(&self) -> bool {
        self.files.is_empty() && self.definitions.is_empty()
    }
}
