use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;
use syntax::types::Type;

#[derive(Default)]
pub(crate) struct ModuleState {
    user_to_string_types: HashSet<String>,
    escape_remap: HashMap<String, String>,
    generic_renames: HashMap<String, String>,
}

impl ModuleState {
    pub(crate) fn record_user_to_string_type(&mut self, type_name: impl Into<String>) {
        self.user_to_string_types.insert(type_name.into());
    }

    pub(crate) fn has_user_to_string(&self, type_name: &str) -> bool {
        self.user_to_string_types.contains(type_name)
    }

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
}

#[derive(Default)]
pub(crate) struct FunctionEmissionState {
    absorbed_ref_generics: HashSet<String>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

impl FunctionEmissionState {
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
