use crate::ir::declarations::GoDeclaration;
use crate::names::packages::{PackageRequirements, PackageUse};
use crate::render::imports::ImportPlan;
use crate::render::output::{OutputCollector, OutputImport};
use crate::state::rejections::Rejections;
use diagnostics::LisetteDiagnostic;
use std::mem;

#[derive(Clone, Default)]
pub(crate) struct FileNamespace {
    imports: ImportPlan,
    requirements: PackageRequirements,
    adapter_declarations: Vec<GoDeclaration>,
    rejections: Rejections,
}

impl FileNamespace {
    pub(crate) fn new(imports: ImportPlan) -> Self {
        Self {
            imports,
            requirements: PackageRequirements::default(),
            adapter_declarations: Vec::new(),
            rejections: Rejections::default(),
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

    pub(crate) fn rejections(&self) -> &Rejections {
        &self.rejections
    }

    pub(crate) fn collect_adapters(&mut self, declarations: Vec<GoDeclaration>) {
        self.adapter_declarations.extend(declarations);
    }
    pub(crate) fn take_adapters(&mut self) -> Vec<GoDeclaration> {
        mem::take(&mut self.adapter_declarations)
    }

    pub(crate) fn finish(
        self,
        source: OutputCollector,
    ) -> (Vec<OutputImport>, String, Vec<LisetteDiagnostic>) {
        let (imports, mut diagnostics) = self.imports.finish(&self.requirements);
        diagnostics.extend(self.rejections.into_diagnostics());
        (imports, source.render(), diagnostics)
    }
}
