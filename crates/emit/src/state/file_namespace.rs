use rustc_hash::FxHashMap as HashMap;

use crate::names::packages::{PackageRequirements, PackageUse};
use crate::output::imports::{ImportBuilder, ImportPlan};
use diagnostics::LisetteDiagnostic;
use ecow::EcoString;
use syntax::ast::ImportAlias;
use syntax::program::File;

pub(crate) struct FileNamespace {
    file_id: u32,
    module_aliases: HashMap<String, String>,
    reverse_aliases: HashMap<String, String>,
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
        let mut module_aliases = HashMap::default();
        let mut reverse_aliases = HashMap::default();
        for import in file.imports() {
            if matches!(import.alias, Some(ImportAlias::Blank(_))) {
                continue;
            }
            let Some(alias) = import.effective_alias(go_package_names) else {
                continue;
            };
            module_aliases.insert(import.name.to_string(), alias.clone());
            reverse_aliases.insert(alias, import.name.to_string());
        }

        Self {
            file_id: file.id,
            module_aliases,
            reverse_aliases,
            imports: ImportPlan::build(file, go_module, unused_imports, go_package_names),
            requirements: PackageRequirements::default(),
        }
    }

    pub(crate) fn file_id(&self) -> u32 {
        self.file_id
    }

    pub(crate) fn module_alias(&self, module: &str) -> Option<&str> {
        self.module_aliases.get(module).map(String::as_str)
    }

    pub(crate) fn module_for_alias(&self, alias: &str) -> Option<&str> {
        self.reverse_aliases.get(alias).map(String::as_str)
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
        go_module_ids: &rustc_hash::FxHashSet<String>,
    ) -> (Vec<(String, String)>, Vec<LisetteDiagnostic>) {
        let mut builder = ImportBuilder::from_plan(self.imports, go_package_names, go_module_ids);
        builder.extend_with_package_uses(&self.requirements);
        builder.build()
    }
}
