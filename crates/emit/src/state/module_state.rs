use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use crate::EnumLayout;
use crate::names::constraints::{GenericConstraintTable, ParamConstraintSet};

#[derive(Default)]
pub(crate) struct ModuleState {
    enum_layouts: RefCell<HashMap<String, Rc<EnumLayout>>>,
    tag_exported_fields: HashSet<String>,
    exported_method_names: HashSet<String>,
    user_to_string_types: HashSet<String>,
    module_aliases: HashMap<String, String>,
    reverse_module_aliases: HashMap<String, String>,
    escape_remap: HashMap<String, String>,
    generic_constraints: GenericConstraintTable,
}

impl ModuleState {
    pub(crate) fn record_enum_layout(&self, enum_id: String, layout: EnumLayout) {
        self.enum_layouts
            .borrow_mut()
            .insert(enum_id, Rc::new(layout));
    }

    pub(crate) fn enum_layout(&self, enum_id: &str) -> Option<Rc<EnumLayout>> {
        self.enum_layouts.borrow().get(enum_id).cloned()
    }

    pub(crate) fn has_enum_layout(&self, enum_id: &str) -> bool {
        self.enum_layouts.borrow().contains_key(enum_id)
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

    pub(crate) fn record_user_to_string_type(&mut self, type_name: impl Into<String>) {
        self.user_to_string_types.insert(type_name.into());
    }

    pub(crate) fn has_user_to_string(&self, type_name: &str) -> bool {
        self.user_to_string_types.contains(type_name)
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

    pub(crate) fn set_generic_constraints(&mut self, table: GenericConstraintTable) {
        self.generic_constraints = table;
    }

    pub(crate) fn generic_constraints_for(&self, symbol: &str) -> Option<&[ParamConstraintSet]> {
        self.generic_constraints.get(symbol)
    }
}

#[derive(Default)]
pub(crate) struct FunctionEmissionState {
    absorbed_ref_generics: HashSet<String>,
    eager_operand_capture: bool,
}

impl FunctionEmissionState {
    pub(crate) fn is_absorbed_ref_generic(&self, name: &str) -> bool {
        self.absorbed_ref_generics.contains(name)
    }

    pub(crate) fn record_absorbed_ref_generic(&mut self, name: impl Into<String>) {
        self.absorbed_ref_generics.insert(name.into());
    }

    pub(crate) fn eager_operand_capture(&self) -> bool {
        self.eager_operand_capture
    }

    pub(crate) fn set_eager_operand_capture(&mut self, value: bool) -> bool {
        std::mem::replace(&mut self.eager_operand_capture, value)
    }
}
