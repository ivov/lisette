use rustc_hash::FxHashSet as HashSet;

use crate::names::go_name::GeneratedPackage;
use crate::output::imports::ImportBuilder;
use crate::types::go_type::GoType;

#[derive(Debug, Clone, Copy, Default)]
struct GeneratedImportSet(u8);

impl GeneratedImportSet {
    fn insert(&mut self, package: GeneratedPackage) {
        self.0 |= 1 << package as u8;
    }

    fn union(&mut self, other: Self) {
        self.0 |= other.0;
    }

    fn iter(self) -> impl Iterator<Item = GeneratedPackage> {
        GeneratedPackage::ALL
            .iter()
            .copied()
            .filter(move |package| self.0 & (1 << *package as u8) != 0)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EmitEffects {
    generated: GeneratedImportSet,
    pub go_imports: Vec<String>,
}

impl EmitEffects {
    pub(crate) fn require_stdlib(&mut self) {
        self.generated.insert(GeneratedPackage::Prelude);
    }

    pub(crate) fn require_fmt(&mut self) {
        self.generated.insert(GeneratedPackage::Fmt);
    }

    pub(crate) fn require_errors(&mut self) {
        self.generated.insert(GeneratedPackage::Errors);
    }

    pub(crate) fn require_slices(&mut self) {
        self.generated.insert(GeneratedPackage::Slices);
    }

    pub(crate) fn require_strings(&mut self) {
        self.generated.insert(GeneratedPackage::Strings);
    }

    pub(crate) fn require_maps(&mut self) {
        self.generated.insert(GeneratedPackage::Maps);
    }

    pub(crate) fn require_json(&mut self) {
        self.generated.insert(GeneratedPackage::Json);
    }

    pub(crate) fn require_cmp(&mut self) {
        self.generated.insert(GeneratedPackage::Cmp);
    }

    pub(crate) fn require_go_import(&mut self, path: impl Into<String>) {
        self.go_imports.push(path.into());
    }

    pub(crate) fn merge_from_go_type(&mut self, go_type: &GoType) {
        if go_type.needs_stdlib {
            self.require_stdlib();
        }
        self.go_imports.extend(go_type.go_imports.iter().cloned());
    }

    pub(crate) fn extend(&mut self, other: &EmitEffects) {
        self.generated.union(other.generated);
        self.go_imports.extend(other.go_imports.iter().cloned());
    }

    pub(crate) fn drain_into(&self, builder: &mut ImportBuilder) {
        let modules: HashSet<String> = self.go_imports.iter().cloned().collect();
        builder.extend_with_modules(&modules);
        for package in self.generated.iter() {
            builder.require_generated(package);
        }
    }
}
