use rustc_hash::FxHashSet as HashSet;

use crate::names::go_name::GeneratedPackage;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageId(String);

impl PackageId {
    pub(crate) fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub(crate) fn path(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageUse {
    package: PackageId,
    qualifier: String,
}

impl PackageUse {
    pub(crate) fn new(path: impl Into<String>, qualifier: impl Into<String>) -> Self {
        Self {
            package: PackageId::new(path),
            qualifier: qualifier.into(),
        }
    }

    pub(crate) fn generated(package: GeneratedPackage) -> Self {
        Self::new(package.path(), package.qualifier())
    }

    pub(crate) fn package(&self) -> &PackageId {
        &self.package
    }

    pub(crate) fn qualifier(&self) -> &str {
        &self.qualifier
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PackageRequirements {
    uses: HashSet<PackageUse>,
}

impl PackageRequirements {
    pub(crate) fn require(&mut self, package: PackageUse) {
        self.uses.insert(package);
    }

    pub(crate) fn require_generated(&mut self, package: GeneratedPackage) {
        self.require(PackageUse::generated(package));
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        self.uses.extend(other.uses.iter().cloned());
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &PackageUse> {
        self.uses.iter()
    }
}
