use rustc_hash::FxHashSet;

use crate::target::names::go_name::GeneratedPackage;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageId(String);

impl PackageId {
    fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub(crate) fn path(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PackageOrigin {
    Generated,
    GoImport,
    Lisette,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageUse {
    package: PackageId,
    qualifier: String,
    origin: PackageOrigin,
}

impl PackageUse {
    pub(crate) fn new(
        path: impl Into<String>,
        qualifier: impl Into<String>,
        origin: PackageOrigin,
    ) -> Self {
        Self {
            package: PackageId::new(path),
            qualifier: qualifier.into(),
            origin,
        }
    }

    pub(crate) fn generated(package: GeneratedPackage) -> Self {
        Self::new(
            package.path(),
            package.qualifier(),
            PackageOrigin::Generated,
        )
    }

    pub(crate) fn origin(&self) -> PackageOrigin {
        self.origin
    }

    pub(crate) fn package(&self) -> &PackageId {
        &self.package
    }

    pub(crate) fn qualifier(&self) -> &str {
        &self.qualifier
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PackageRequirements {
    uses: FxHashSet<PackageUse>,
}

impl PackageRequirements {
    pub(crate) fn require(&mut self, package: PackageUse) {
        self.uses.insert(package);
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        self.uses.extend(other.uses.iter().cloned());
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &PackageUse> {
        self.uses.iter()
    }
}
