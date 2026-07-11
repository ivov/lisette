use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use crate::EnumLayout;

#[derive(Default)]
pub(crate) struct ModuleState {
    enum_layouts: RefCell<HashMap<u32, HashMap<String, Rc<EnumLayout>>>>,
    tag_exported_fields: HashSet<String>,
    exported_method_names: HashSet<String>,
    user_to_string_types: HashSet<String>,
    escape_remap: HashMap<String, String>,
    generic_renames: HashMap<String, String>,
}

impl ModuleState {
    pub(crate) fn record_enum_layout(&self, file_id: u32, enum_id: String, layout: EnumLayout) {
        self.enum_layouts
            .borrow_mut()
            .entry(file_id)
            .or_default()
            .insert(enum_id, Rc::new(layout));
    }

    pub(crate) fn enum_layout(&self, file_id: u32, enum_id: &str) -> Option<Rc<EnumLayout>> {
        self.enum_layouts
            .borrow()
            .get(&file_id)?
            .get(enum_id)
            .cloned()
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
}

impl FunctionEmissionState {
    pub(crate) fn is_absorbed_ref_generic(&self, name: &str) -> bool {
        self.absorbed_ref_generics.contains(name)
    }

    pub(crate) fn record_absorbed_ref_generic(&mut self, name: impl Into<String>) {
        self.absorbed_ref_generics.insert(name.into());
    }
}
