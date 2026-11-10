use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::go_names::is_go_reserved_word;

use super::ScopeState;
use crate::names::go_name;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::state::package_state::PackageState;

#[derive(Default)]
pub(super) struct LocalNames {
    issued: HashSet<String>,
    generated: HashMap<LocalId, String>,
    struct_update_copies: HashSet<LocalId>,
}

impl ScopeState {
    pub(crate) fn fresh_go_name(
        &mut self,
        hint: Option<&str>,
        package: &PackageState,
    ) -> GoIdentifier {
        let base = hint.unwrap_or("tmp").to_string();
        let candidate = go_name::fresh_suffixed(&base, |name| self.is_go_name_taken(name, package));
        self.names.issued.insert(candidate.clone());
        let id = self.new_local_id();
        self.names.generated.insert(id, base);
        GoIdentifier::local(candidate, id)
    }

    pub(crate) fn generated_base(&self, id: LocalId) -> Option<&str> {
        self.names.generated.get(&id).map(String::as_str)
    }

    pub(crate) fn mark_struct_update_copy(&mut self, id: LocalId) {
        self.names.struct_update_copies.insert(id);
    }

    pub(crate) fn is_struct_update_copy(&self, id: LocalId) -> bool {
        self.names.struct_update_copies.contains(&id)
    }

    pub(crate) fn fresh_binding_go_name(&mut self, hint: &str, package: &PackageState) -> String {
        let candidate = go_name::fresh_suffixed(hint, |name| self.is_go_name_taken(name, package));
        self.names.issued.insert(candidate.clone());
        candidate
    }

    pub(crate) fn helper_local(&mut self, hint: &str, package: &PackageState) -> GoIdentifier {
        let name = go_name::fresh_suffixed(hint, |name| {
            is_go_reserved_word(name)
                || self.declares_type_param(name)
                || package.is_package_block_name(name)
        });
        self.new_local_identifier(name)
    }

    pub(crate) fn reserve_go_name(&mut self, go_name: &str) {
        self.names.issued.insert(go_name.to_string());
    }

    pub(crate) fn settle_generated_names(
        &self,
        present: &HashMap<LocalId, String>,
        pinned: &HashSet<String>,
        package: &PackageState,
    ) -> HashMap<LocalId, String> {
        // A source binding that never reached the body frees its name.
        let mut taken = pinned.clone();
        taken.extend(
            present
                .iter()
                .filter(|(id, _)| !self.names.generated.contains_key(id))
                .map(|(_, name)| name.clone()),
        );
        let mut locals: Vec<_> = self.names.generated.iter().collect();
        locals.sort_by_key(|(id, _)| id.0);
        let mut settled: HashMap<LocalId, String> = HashMap::default();
        for (id, base) in locals {
            let Some(current) = present.get(id) else {
                continue;
            };
            if pinned.contains(current) {
                continue;
            }
            let name = go_name::fresh_suffixed(base, |candidate| {
                taken.contains(candidate)
                    || is_go_reserved_word(candidate)
                    || self.has_binding_for_go_name(candidate)
                    || self.declares_type_param(candidate)
                    // Type strings can name import qualifiers the body never shows.
                    || package.is_import_qualifier(candidate)
            });
            // Raising a suffix would renumber a name for no reader's benefit.
            if name != *base || name == *current {
                taken.insert(current.clone());
                continue;
            }
            taken.insert(name.clone());
            settled.insert(*id, name);
        }
        settled
    }

    fn is_go_name_taken(&self, go_name: &str, package: &PackageState) -> bool {
        is_go_reserved_word(go_name)
            || self.names.issued.contains(go_name)
            || self.has_binding_for_go_name(go_name)
            || self.is_go_name_declared(go_name)
            || package.is_package_block_name(go_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_name_skips_bound_go_name() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();
        scope.bind_source("value", &[], "tmp");

        assert_eq!(scope.fresh_go_name(None, &no_package).spelling(), "tmp2");
    }

    #[test]
    fn fresh_name_prefers_the_bare_hint_then_numbers_it() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();

        assert_eq!(
            scope.fresh_go_name(Some("value"), &no_package).spelling(),
            "value"
        );
        assert_eq!(
            scope.fresh_go_name(Some("value"), &no_package).spelling(),
            "value2"
        );
        assert_eq!(
            scope.fresh_go_name(Some("other"), &no_package).spelling(),
            "other"
        );
    }

    #[test]
    fn fresh_name_never_spells_a_go_reserved_word() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();

        assert_eq!(
            scope.fresh_go_name(Some("range"), &no_package).spelling(),
            "range2"
        );
        assert_eq!(
            scope.fresh_go_name(Some("len"), &no_package).spelling(),
            "len2"
        );
    }

    #[test]
    fn opaque_reference_keeps_a_generated_spelling() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();
        scope.reserve_go_name("value");
        let generated = scope.fresh_go_name(Some("value"), &no_package);
        assert_eq!(generated.spelling(), "value2");
        let present = HashMap::from_iter([(generated.id().unwrap(), generated.to_string())]);
        let pinned = HashSet::from_iter([generated.to_string()]);
        assert!(
            scope
                .settle_generated_names(&present, &pinned, &no_package)
                .is_empty()
        );
    }

    #[test]
    fn fresh_names_skip_package_block_names_without_issuing_them() {
        let mut scope = ScopeState::new();
        let mut package = PackageState::default();
        package.record_package_block_names(
            HashSet::from_iter(["value".to_string()]),
            HashSet::from_iter(["x".to_string()]),
        );

        assert_eq!(
            scope.fresh_go_name(Some("value"), &package).spelling(),
            "value2"
        );
        assert_eq!(scope.names.generated.len(), 1);
        assert_eq!(scope.fresh_binding_go_name("x", &package), "x2");
    }

    #[test]
    fn generated_local_does_not_settle_onto_an_import_qualifier() {
        let mut scope = ScopeState::new();
        let mut package = PackageState::default();
        package
            .record_package_block_names(HashSet::default(), HashSet::from_iter(["i".to_string()]));
        let generated = scope.fresh_go_name(Some("i"), &package);
        assert_eq!(generated.spelling(), "i2");
        let present = HashMap::from_iter([(generated.id().unwrap(), generated.to_string())]);
        assert!(
            scope
                .settle_generated_names(&present, &HashSet::default(), &package)
                .is_empty()
        );
    }

    #[test]
    fn declaration_scope_isolates_locals_and_restores_outer_scope() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();
        scope.reserve_go_name("value");

        let outer = scope.begin_declaration();
        assert_eq!(
            scope.fresh_go_name(Some("value"), &no_package).spelling(),
            "value"
        );
        assert_eq!(
            scope.fresh_go_name(Some("tmp"), &no_package).spelling(),
            "tmp"
        );
        scope.end_declaration(outer);

        assert_eq!(
            scope.fresh_go_name(Some("value"), &no_package).spelling(),
            "value2"
        );
        assert_eq!(
            scope.fresh_go_name(Some("tmp"), &no_package).spelling(),
            "tmp"
        );
    }

    #[test]
    fn retired_generated_local_does_not_free_a_distinct_source_local() {
        let mut scope = ScopeState::new();
        let package = PackageState::default();
        scope.reserve_go_name("value");
        let retired = scope.fresh_go_name(Some("value"), &package);
        let generated = scope.fresh_go_name(Some("value2"), &package);
        let source = scope.new_local_identifier(retired.spelling());
        assert_ne!(source.id(), retired.id());
        let present = HashMap::from_iter([
            (source.id().unwrap(), source.to_string()),
            (generated.id().unwrap(), generated.to_string()),
        ]);
        assert!(
            scope
                .settle_generated_names(&present, &HashSet::default(), &package)
                .is_empty()
        );
    }

    #[test]
    fn earlier_generated_local_settles_first() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();
        scope.reserve_go_name("tmp");
        let first = scope.fresh_go_name(None, &no_package);
        let second = scope.fresh_go_name(None, &no_package);
        assert_eq!((first.spelling(), second.spelling()), ("tmp2", "tmp3"));
        let first_id = first.id().unwrap();
        let present = HashMap::from_iter([
            (first.id().unwrap(), first.to_string()),
            (second.id().unwrap(), second.to_string()),
        ]);
        let settled = scope.settle_generated_names(&present, &HashSet::default(), &no_package);
        assert_eq!(settled.len(), 1);
        assert_eq!(settled.get(&first_id).map(String::as_str), Some("tmp"));
    }
}
