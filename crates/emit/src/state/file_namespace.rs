use crate::names::packages::{PackageRequirements, PackageUse};
use crate::output::OutputImport;
use crate::output::imports::ImportPlan;
use diagnostics::LisetteDiagnostic;

#[derive(Default)]
pub(crate) struct FileNamespace {
    imports: ImportPlan,
    requirements: PackageRequirements,
}

impl FileNamespace {
    pub(crate) fn new(imports: ImportPlan) -> Self {
        Self {
            imports,
            requirements: PackageRequirements::default(),
        }
    }

    pub(crate) fn import_qualifier(&self, package: &str) -> Option<&str> {
        self.imports.import_qualifier(package)
    }

    pub(crate) fn package_for_alias(&self, alias: &str) -> Option<&str> {
        self.imports.package_for_alias(alias)
    }

    pub(crate) fn require(&mut self, package: PackageUse) {
        self.requirements.require(package);
    }

    pub(crate) fn absorb(&mut self, requirements: &PackageRequirements) {
        self.requirements.extend(requirements);
    }

    pub(crate) fn finish(self) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        self.imports.finish(&self.requirements)
    }
}
