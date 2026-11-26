use crate::target::names::go_name::GoExport;
use crate::target::names::receiver::ReceiverGenerics;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::borrow::Cow;
use syntax::ast::Generic;
use syntax::go_names::is_go_reserved_word;

use crate::target::names::go_name;

#[derive(Clone, Default)]
pub(crate) struct PackageNames {
    escape_remap: FxHashMap<String, String>,
    generic_renames: FxHashMap<String, String>,
    declared_names: FxHashSet<String>,
    import_qualifiers: FxHashSet<String>,
}

impl PackageNames {
    pub(crate) fn generic_go_name<'argument>(
        &'argument self,
        source_name: &'argument str,
    ) -> Cow<'argument, str> {
        match self.generic_rename(source_name) {
            Some(renamed) => Cow::Borrowed(renamed),
            None => go_name::escape_type_name(source_name),
        }
    }

    pub(crate) fn receiver_generics(&self, generics: &[Generic]) -> ReceiverGenerics {
        ReceiverGenerics(
            generics
                .iter()
                .map(|generic| self.generic_go_name(&generic.name).into_owned())
                .collect(),
        )
    }

    pub(crate) fn free_function_go_name(&self, name: &str, export: GoExport) -> String {
        match export {
            GoExport::Exported => go_name::snake_to_camel(name),
            GoExport::Unexported => self
                .escape_remap(name)
                .map(str::to_string)
                .unwrap_or_else(|| go_name::escape_reserved(name).into_owned()),
        }
    }

    pub(crate) fn set_block_names(
        &mut self,
        declared_names: FxHashSet<String>,
        import_qualifiers: FxHashSet<String>,
    ) {
        self.declared_names = declared_names;
        self.import_qualifiers = import_qualifiers;
    }

    pub(crate) fn set_escape_remap(&mut self, escape_remap: FxHashMap<String, String>) {
        self.escape_remap = escape_remap;
    }

    pub(crate) fn set_generic_renames(&mut self, generic_renames: FxHashMap<String, String>) {
        self.generic_renames = generic_renames;
    }

    pub(crate) fn escape_remap(&self, lisette_name: &str) -> Option<&str> {
        self.escape_remap.get(lisette_name).map(String::as_str)
    }

    pub(crate) fn generic_rename(&self, source_name: &str) -> Option<&str> {
        self.generic_renames.get(source_name).map(String::as_str)
    }

    pub(crate) fn declares_name(&self, go_name: &str) -> bool {
        self.declared_names.contains(go_name)
    }

    pub(crate) fn is_package_block_name(&self, go_name: &str) -> bool {
        self.declared_names.contains(go_name) || self.import_qualifiers.contains(go_name)
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
