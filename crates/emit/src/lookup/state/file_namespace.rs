use crate::lookup::state::imports::{ImportPlan, OutputImport};
use crate::lookup::state::rejections::Rejections;
use crate::target::ir::declarations::GoDeclaration;
use crate::target::names::go_name;
use crate::target::names::packages::{PackageRequirements, PackageUse};
use diagnostics::LisetteDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::mem;

#[derive(Clone)]
pub(crate) struct FileNamespace {
    imports: ImportPlan,
    requirements: PackageRequirements,
    adapter_declarations: Vec<GoDeclaration>,
    rejections: Rejections,
    claimed_qualifiers: RefCell<FxHashMap<String, String>>,
    generated_collisions: FxHashSet<String>,
}

impl FileNamespace {
    pub(crate) fn new(imports: ImportPlan, generated_collisions: FxHashSet<String>) -> Self {
        Self {
            imports,
            requirements: PackageRequirements::default(),
            adapter_declarations: Vec::new(),
            rejections: Rejections::default(),
            claimed_qualifiers: RefCell::default(),
            generated_collisions,
        }
    }

    pub(crate) fn import_qualifier(&self, package: &str) -> Option<&str> {
        self.imports.import_qualifier(package)
    }

    pub(crate) fn is_claimed_by_other_path(&self, qualifier: &str, path: &str) -> bool {
        self.imports.has_qualifier_for_other_path(qualifier, path)
            || (self.generated_collisions.contains(qualifier)
                && go_name::is_generated_qualifier_for_other_path(qualifier, path))
            || self
                .claimed_qualifiers
                .borrow()
                .get(qualifier)
                .is_some_and(|claimed_path| claimed_path != path)
    }

    pub(crate) fn claim_qualifier(&self, qualifier: &str, path: &str) {
        let mut claimed = self.claimed_qualifiers.borrow_mut();
        if !claimed.contains_key(qualifier) {
            claimed.insert(qualifier.to_string(), path.to_string());
        }
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

    pub(crate) fn finish(self) -> (Vec<OutputImport>, Vec<LisetteDiagnostic>) {
        let (imports, mut diagnostics) = self.imports.finish(&self.requirements);
        diagnostics.extend(self.rejections.into_diagnostics());
        (imports, diagnostics)
    }
}
