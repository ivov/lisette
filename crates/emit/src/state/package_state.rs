use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;
use syntax::types::Type;

#[derive(Default)]
pub(crate) struct PackageState {
    escape_remap: HashMap<String, String>,
    generic_renames: HashMap<String, String>,
    go_const_bindings: HashSet<String>,
}

impl PackageState {
    pub(crate) fn record_escape_remap(
        &mut self,
        lisette_name: impl Into<String>,
        go_name: impl Into<String>,
    ) {
        self.escape_remap
            .insert(lisette_name.into(), go_name.into());
    }

    pub(crate) fn escape_remap(&self, lisette_name: &str) -> Option<&str> {
        self.escape_remap.get(lisette_name).map(String::as_str)
    }

    pub(crate) fn record_generic_rename(
        &mut self,
        source_name: impl Into<String>,
        go_name: impl Into<String>,
    ) {
        self.generic_renames
            .insert(source_name.into(), go_name.into());
    }

    pub(crate) fn generic_rename(&self, source_name: &str) -> Option<&str> {
        self.generic_renames.get(source_name).map(String::as_str)
    }

    pub(crate) fn extend_go_const_bindings(&mut self, names: impl IntoIterator<Item = String>) {
        self.go_const_bindings.extend(names);
    }

    pub(crate) fn is_go_const_binding(&self, lisette_name: &str) -> bool {
        self.go_const_bindings.contains(lisette_name)
    }
}

pub(crate) struct FunctionEmissionContext {
    absorbed_ref_generics: HashSet<String>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

impl FunctionEmissionContext {
    pub(crate) fn for_function(
        generic_context: &[(EcoString, Vec<Type>)],
        absorbed_ref_generics: HashSet<String>,
    ) -> Self {
        Self {
            absorbed_ref_generics,
            generic_context: generic_context.to_vec(),
        }
    }

    pub(crate) fn is_absorbed_ref_generic(&self, name: &str) -> bool {
        self.absorbed_ref_generics.contains(name)
    }

    pub(crate) fn generic_context(&self) -> &[(EcoString, Vec<Type>)] {
        &self.generic_context
    }
}
