use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;
use std::cell::RefCell;
use syntax::ast::Span;
use syntax::go_names::is_go_reserved_word;
use syntax::program::ValueUseKey;

use crate::names::go_name;

#[derive(Clone, Default)]
pub(crate) struct PackageState {
    escape_remap: HashMap<String, String>,
    generic_renames: HashMap<String, String>,
    declared_names: HashSet<String>,
    import_qualifiers: HashSet<String>,
    applied_conversions: RefCell<HashSet<ValueUseKey>>,
    dropped_code: RefCell<Vec<Span>>,
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

    pub(crate) fn record_applied_conversion(&self, key: ValueUseKey) {
        if cfg!(debug_assertions) {
            self.applied_conversions.borrow_mut().insert(key);
        }
    }

    pub(crate) fn applied_conversions(&self) -> HashSet<ValueUseKey> {
        self.applied_conversions.borrow().clone()
    }

    pub(crate) fn record_dropped_code(&self, span: Span) {
        if cfg!(debug_assertions) {
            self.dropped_code.borrow_mut().push(span);
        }
    }

    pub(crate) fn dropped_code(&self) -> Vec<Span> {
        self.dropped_code.borrow().clone()
    }

    pub(crate) fn generic_rename(&self, source_name: &str) -> Option<&str> {
        self.generic_renames.get(source_name).map(String::as_str)
    }

    pub(crate) fn record_package_block_names(
        &mut self,
        declared_names: HashSet<String>,
        import_qualifiers: HashSet<String>,
    ) {
        self.declared_names = declared_names;
        self.import_qualifiers = import_qualifiers;
    }

    pub(crate) fn declares_name(&self, go_name: &str) -> bool {
        self.declared_names.contains(go_name)
    }

    pub(crate) fn is_package_block_name(&self, go_name: &str) -> bool {
        self.declared_names.contains(go_name) || self.import_qualifiers.contains(go_name)
    }

    pub(crate) fn is_import_qualifier(&self, go_name: &str) -> bool {
        self.import_qualifiers.contains(go_name)
    }

    pub(crate) fn fresh_package_name(
        &self,
        base: &str,
        also_taken: impl Fn(&str) -> bool,
    ) -> String {
        go_name::fresh_suffixed(base, |candidate| {
            self.is_package_block_name(candidate)
                || is_go_reserved_word(candidate)
                || also_taken(candidate)
        })
    }
}
