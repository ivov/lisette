use crate::names::packages::{PackageRequirements, PackageUse};
use crate::output::imports::ImportPlan;
use crate::output::{OutputCollector, OutputImport};
use diagnostics::LisetteDiagnostic;

#[derive(Default)]
pub(crate) struct FileNamespace {
    imports: ImportPlan,
    requirements: PackageRequirements,
    adapter_declarations: Vec<String>,
}

impl FileNamespace {
    pub(crate) fn new(imports: ImportPlan) -> Self {
        Self {
            imports,
            requirements: PackageRequirements::default(),
            adapter_declarations: Vec::new(),
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

    pub(crate) fn collect_adapter(&mut self, declaration: String) {
        self.adapter_declarations.push(declaration);
    }

    pub(crate) fn finish(
        self,
        mut source: OutputCollector,
    ) -> (Vec<OutputImport>, String, Vec<LisetteDiagnostic>) {
        for declaration in self.adapter_declarations {
            source.collect_with_blank(declaration);
        }
        let (imports, diagnostics) = self.imports.finish(&self.requirements);
        (imports, source.render(), diagnostics)
    }
}
