use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::go_names::is_go_reserved_word;

use super::ScopeState;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::state::package_state::PackageState;

#[derive(Default)]
pub(super) struct LocalNames {
    issued: HashSet<String>,
    generated: HashMap<String, GeneratedLocal>,
}

struct GeneratedLocal {
    base: String,
    id: LocalId,
}

impl ScopeState {
    pub(crate) fn fresh_go_name(&mut self, hint: Option<&str>, package: &PackageState) -> String {
        let base = hint.unwrap_or("tmp").to_string();
        let candidate = self.free_name_from(&base, |name| self.is_go_name_taken(name, package));
        self.names.issued.insert(candidate.clone());
        let id = self.new_local_id();
        self.names
            .generated
            .insert(candidate.clone(), GeneratedLocal { base, id });
        candidate
    }

    pub(crate) fn generated_local_id(&self, name: &str) -> Option<LocalId> {
        self.names.generated.get(name).map(|local| local.id)
    }

    pub(crate) fn generated_identifier(&self, name: &str) -> GoIdentifier {
        GoIdentifier::local(
            name.to_string(),
            self.generated_local_id(name)
                .expect("generated name has a local ID"),
        )
    }

    pub(crate) fn fresh_binding_go_name(&mut self, hint: &str, package: &PackageState) -> String {
        let candidate = self.free_name_from(hint, |name| self.is_go_name_taken(name, package));
        self.names.issued.insert(candidate.clone());
        candidate
    }

    fn free_name_from(&self, base: &str, taken: impl Fn(&str) -> bool) -> String {
        let mut candidate = base.to_string();
        let mut suffix = 0;
        while taken(&candidate) {
            suffix += 1;
            candidate = format!("{base}_{suffix}");
        }
        candidate
    }

    pub(crate) fn reserve_go_name(&mut self, go_name: &str) {
        self.names.issued.insert(go_name.to_string());
    }

    pub(crate) fn settle_generated_names(
        &self,
        present: &HashSet<String>,
        pinned: &HashSet<String>,
        package: &PackageState,
    ) -> HashMap<LocalId, String> {
        // A source binding that never reached the body frees its name.
        let mut taken: HashSet<String> = HashSet::default();
        taken.extend(
            present
                .iter()
                .filter(|name| !self.names.generated.contains_key(name.as_str()))
                .cloned(),
        );
        let mut locals: Vec<(&String, &GeneratedLocal)> = self.names.generated.iter().collect();
        locals.sort_by_key(|(_, local)| local.id.0);
        let mut settled: HashMap<LocalId, String> = HashMap::default();
        for (current, local) in locals {
            if !present.contains(current) || pinned.contains(current) {
                continue;
            }
            let name = self.free_name_from(&local.base, |candidate| {
                taken.contains(candidate)
                    || is_go_reserved_word(candidate)
                    || self.has_binding_for_go_name(candidate)
                    || self.declares_type_param(candidate)
                    // Type strings can name import qualifiers the body never shows.
                    || package.is_import_qualifier(candidate)
            });
            // Raising a suffix would renumber a name for no reader's benefit.
            if name != local.base || name == *current {
                taken.insert(current.clone());
                continue;
            }
            taken.insert(name.clone());
            settled.insert(local.id, name);
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

        assert_eq!(scope.fresh_go_name(None, &no_package), "tmp_1");
    }

    #[test]
    fn fresh_name_prefers_the_bare_hint_then_numbers_it() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();

        assert_eq!(scope.fresh_go_name(Some("value"), &no_package), "value");
        assert_eq!(scope.fresh_go_name(Some("value"), &no_package), "value_1");
        assert_eq!(scope.fresh_go_name(Some("other"), &no_package), "other");
    }

    #[test]
    fn fresh_name_never_spells_a_go_reserved_word() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();

        assert_eq!(scope.fresh_go_name(Some("range"), &no_package), "range_1");
        assert_eq!(scope.fresh_go_name(Some("len"), &no_package), "len_1");
    }

    #[test]
    fn opaque_reference_keeps_a_generated_spelling() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();
        scope.reserve_go_name("value");
        let generated = scope.fresh_go_name(Some("value"), &no_package);
        assert_eq!(generated, "value_1");
        let present = HashSet::from_iter([generated.clone()]);
        let pinned = HashSet::from_iter([generated]);
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

        assert_eq!(scope.fresh_go_name(Some("value"), &package), "value_1");
        assert_eq!(scope.generated_local_id("value"), None);
        assert_eq!(scope.fresh_binding_go_name("x", &package), "x_1");
    }

    #[test]
    fn generated_local_does_not_settle_onto_an_import_qualifier() {
        let mut scope = ScopeState::new();
        let mut package = PackageState::default();
        package
            .record_package_block_names(HashSet::default(), HashSet::from_iter(["i".to_string()]));
        let generated = scope.fresh_go_name(Some("i"), &package);
        assert_eq!(generated, "i_1");
        let present = HashSet::from_iter([generated]);
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
        assert_eq!(scope.fresh_go_name(Some("value"), &no_package), "value");
        assert_eq!(scope.fresh_go_name(Some("tmp"), &no_package), "tmp");
        scope.end_declaration(outer);

        assert_eq!(scope.fresh_go_name(Some("value"), &no_package), "value_1");
        assert_eq!(scope.fresh_go_name(Some("tmp"), &no_package), "tmp");
    }

    #[test]
    fn earlier_generated_local_settles_first() {
        let mut scope = ScopeState::new();
        let no_package = PackageState::default();
        scope.reserve_go_name("tmp");
        let first = scope.fresh_go_name(None, &no_package);
        let second = scope.fresh_go_name(None, &no_package);
        assert_eq!((first.as_str(), second.as_str()), ("tmp_1", "tmp_2"));
        let first_id = scope.generated_local_id(&first).unwrap();
        let present = HashSet::from_iter([first, second]);
        let settled = scope.settle_generated_names(&present, &HashSet::default(), &no_package);
        assert_eq!(settled.len(), 1);
        assert_eq!(settled.get(&first_id).map(String::as_str), Some("tmp"));
    }
}
