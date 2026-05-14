use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use syntax::ast::Generic;

use crate::EnumLayout;

#[derive(Default)]
pub(crate) struct ModuleState {
    enum_layouts: HashMap<String, EnumLayout>,
    /// Key is `TypeId.field_name`; capitalization must match the struct
    /// definition because Go cares about exported-vs-private field casing.
    tag_exported_fields: HashSet<String>,
    exported_method_names: HashSet<String>,
    /// Multiple impl blocks with the same receiver must contribute their
    /// bounds because Go requires generic constraints on the type definition
    /// itself.
    impl_bounds: HashMap<String, Vec<Generic>>,
    /// Receivers with both constrained and unconstrained impl blocks need
    /// special-cased generic-arg emission.
    unconstrained_impl_receivers: HashSet<String>,
    module_aliases: HashMap<String, String>,
    reverse_module_aliases: HashMap<String, String>,
    escape_remap: HashMap<String, String>,
}

impl ModuleState {
    pub(crate) fn record_enum_layout(&mut self, enum_id: String, layout: EnumLayout) {
        self.enum_layouts.insert(enum_id, layout);
    }

    pub(crate) fn enum_layout(&self, enum_id: &str) -> Option<&EnumLayout> {
        self.enum_layouts.get(enum_id)
    }

    pub(crate) fn has_enum_layout(&self, enum_id: &str) -> bool {
        self.enum_layouts.contains_key(enum_id)
    }

    pub(crate) fn record_tag_exported_field(&mut self, key: String) {
        self.tag_exported_fields.insert(key);
    }

    pub(crate) fn is_tag_exported_field(&self, key: &str) -> bool {
        self.tag_exported_fields.contains(key)
    }

    pub(crate) fn record_exported_method_name(&mut self, name: impl Into<String>) {
        self.exported_method_names.insert(name.into());
    }

    pub(crate) fn has_local_exported_method_name(&self, name: &str) -> bool {
        self.exported_method_names.contains(name)
    }

    /// Merge new bounds into an existing impl_bounds entry, or insert fresh.
    /// Multiple impl blocks with the same receiver must contribute their bounds
    /// because Go requires generic constraints on the type definition itself.
    pub(crate) fn record_impl_bounds(&mut self, receiver_name: &str, generics: &[Generic]) {
        let Some(existing_generics) = self.impl_bounds.get_mut(receiver_name) else {
            self.impl_bounds
                .insert(receiver_name.to_string(), generics.to_vec());
            return;
        };
        for new_gen in generics {
            let Some(existing_gen) = existing_generics
                .iter_mut()
                .find(|g| g.name == new_gen.name)
            else {
                continue;
            };
            for bound in &new_gen.bounds {
                if !existing_gen.bounds.contains(bound) {
                    existing_gen.bounds.push(bound.clone());
                }
            }
        }
    }

    pub(crate) fn impl_bounds(&self, type_name: &str) -> Option<&[Generic]> {
        self.impl_bounds.get(type_name).map(Vec::as_slice)
    }

    pub(crate) fn record_unconstrained_impl_receiver(&mut self, receiver_name: impl Into<String>) {
        self.unconstrained_impl_receivers
            .insert(receiver_name.into());
    }

    pub(crate) fn has_unconstrained_impl_receiver(&self, type_name: &str) -> bool {
        self.unconstrained_impl_receivers.contains(type_name)
    }

    pub(crate) fn record_module_alias(
        &mut self,
        module: impl Into<String>,
        alias: impl Into<String>,
    ) {
        let module = module.into();
        let alias = alias.into();
        self.module_aliases.insert(module.clone(), alias.clone());
        self.reverse_module_aliases.insert(alias, module);
    }

    pub(crate) fn module_alias(&self, module: &str) -> Option<&str> {
        self.module_aliases.get(module).map(String::as_str)
    }

    pub(crate) fn module_for_alias(&self, alias: &str) -> Option<&str> {
        self.reverse_module_aliases.get(alias).map(String::as_str)
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
}

#[derive(Default)]
pub(crate) struct FunctionEmissionState {
    absorbed_ref_generics: HashSet<String>,
}

impl FunctionEmissionState {
    pub(crate) fn is_absorbed_ref_generic(&self, name: &str) -> bool {
        self.absorbed_ref_generics.contains(name)
    }

    pub(crate) fn record_absorbed_ref_generic(&mut self, name: impl Into<String>) {
        self.absorbed_ref_generics.insert(name.into());
    }
}
