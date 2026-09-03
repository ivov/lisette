use rustc_hash::FxHashMap as HashMap;

use crate::names::packages::{PackageRequirements, PackageUse};
use crate::output::OutputImport;
use crate::output::imports::{ImportBuilder, ImportPlan};
use diagnostics::LisetteDiagnostic;
use ecow::EcoString;
use syntax::program::File;

pub(crate) struct FileNamespace {
    imports: ImportPlan,
    requirements: PackageRequirements,
}

impl FileNamespace {
    pub(crate) fn build(
        file: &File,
        go_module: &str,
        unused_imports: &rustc_hash::FxHashSet<EcoString>,
        go_package_names: &HashMap<String, String>,
    ) -> Self {
        Self {
            imports: ImportPlan::build(file, go_module, unused_imports, go_package_names),
            requirements: PackageRequirements::default(),
        }
    }

    pub(crate) fn package_alias(&self, package: &str) -> Option<&str> {
        self.imports.package_alias(package)
    }

    pub(crate) fn package_for_alias(&self, alias: &str) -> Option<&str> {
        self.imports.package_for_alias(alias)
    }

    pub(crate) fn reference(&mut self, package: PackageUse) -> String {
        let qualifier = package.qualifier().to_string();
        self.requirements.require(package);
        qualifier
    }

    pub(crate) fn require(&mut self, package: PackageUse) {
        self.requirements.require(package);
    }

    pub(crate) fn absorb(&mut self, requirements: &PackageRequirements) {
        self.requirements.extend(requirements);
    }

    pub(crate) fn finish(
        self,
        go_package_names: &HashMap<String, String>,
        go_package_ids: &rustc_hash::FxHashSet<String>,
    ) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        let mut builder = ImportBuilder::from_plan(self.imports, go_package_names, go_package_ids);
        builder.extend_with_package_uses(&self.requirements);
        builder.build()
    }
}
