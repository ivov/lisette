use rustc_hash::{FxHashMap, FxHashSet};
use syntax::go_names::is_go_reserved_word;

use super::scope::ScopeState;
use crate::lookup::facts::package_names::PackageNames;
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::names::go_name;

#[derive(Clone, Default)]
pub(super) struct LocalNames {
    issued: FxHashSet<String>,
    generated: FxHashMap<LocalId, String>,
    struct_update_copies: FxHashSet<LocalId>,
}

impl ScopeState {
    pub(crate) fn fresh_go_name(
        &mut self,
        hint: Option<&str>,
        package: &PackageNames,
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

    pub(crate) fn fresh_binding_go_name(&mut self, hint: &str, package: &PackageNames) -> String {
        let candidate = go_name::fresh_suffixed(hint, |name| self.is_go_name_taken(name, package));
        self.names.issued.insert(candidate.clone());
        candidate
    }

    pub(crate) fn new_unreserved_local(
        &mut self,
        hint: impl Into<GoIdentifier>,
        package: &PackageNames,
    ) -> GoIdentifier {
        let mut name = self.new_local_identifier(hint);
        let free = go_name::fresh_suffixed(name.spelling(), |candidate| {
            self.is_reserved_spelling(candidate, package)
        });
        *name.spelling_mut() = free;
        name
    }

    pub(crate) fn reserve_go_name(&mut self, go_name: &str) {
        self.names.issued.insert(go_name.to_string());
    }

    pub(crate) fn settle_generated_names(
        &self,
        present: &FxHashMap<LocalId, String>,
        pinned: &FxHashSet<String>,
        package: &PackageNames,
    ) -> FxHashMap<LocalId, String> {
        let mut taken = pinned.clone();
        taken.extend(
            present
                .iter()
                .filter(|(id, _)| !self.names.generated.contains_key(id))
                .map(|(_, name)| name.clone()),
        );
        let mut locals: Vec<_> = self.names.generated.iter().collect();
        locals.sort_by_key(|(id, _)| id.0);
        let mut settled: FxHashMap<LocalId, String> = FxHashMap::default();
        for (id, base) in locals {
            let Some(current) = present.get(id) else {
                continue;
            };
            if pinned.contains(current) {
                continue;
            }
            let name = self.fresh_spelling(base, &taken, package);

            if name != *base || name == *current {
                taken.insert(current.clone());
                continue;
            }
            taken.insert(name.clone());
            settled.insert(*id, name);
        }
        settled
    }

    pub(crate) fn restore_builtin_names(
        &self,
        present: &FxHashMap<LocalId, String>,
        pinned: &FxHashSet<String>,
        settled: &mut FxHashMap<LocalId, String>,
        package: &PackageNames,
    ) {
        let mut taken: FxHashSet<String> = pinned
            .iter()
            .chain(present.values())
            .chain(settled.values())
            .cloned()
            .collect();

        let mut locals: Vec<_> = present
            .iter()
            .filter(|(id, _)| !settled.contains_key(id))
            .collect();
        locals.sort_by_key(|(id, _)| id.0);
        for (id, spelling) in locals {
            let Some(builtin) = go_name::escaped_shadowable_builtin(spelling) else {
                continue;
            };
            if pinned.contains(spelling)
                || taken.contains(builtin)
                || self.is_declared_outside_body(builtin, package)
            {
                continue;
            }
            taken.insert(builtin.to_string());
            settled.insert(*id, builtin.to_string());
        }
    }

    pub(crate) fn respell_locals(
        &self,
        respelled: &[LocalId],
        present: &FxHashMap<LocalId, String>,
        mut taken: FxHashSet<String>,
        package: &PackageNames,
    ) -> FxHashMap<LocalId, String> {
        taken.extend(present.values().cloned());
        let mut settled = FxHashMap::default();
        for id in respelled {
            let current = present.get(id).expect("respelled local is present");
            let base = self.generated_base(*id).unwrap_or(current);
            let name = self.fresh_spelling(base, &taken, package);
            taken.insert(name.clone());
            settled.insert(*id, name);
        }
        settled
    }

    pub(crate) fn locals_named_outside_body(
        &self,
        present: &FxHashMap<LocalId, String>,
        qualifiers: &FxHashSet<String>,
        package: &PackageNames,
    ) -> Vec<LocalId> {
        present
            .iter()
            .filter(|(_, spelling)| {
                qualifiers.contains(*spelling) || self.is_declared_outside_body(spelling, package)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    fn fresh_spelling(
        &self,
        base: &str,
        taken: &FxHashSet<String>,
        package: &PackageNames,
    ) -> String {
        go_name::fresh_suffixed(base, |candidate| {
            taken.contains(candidate) || self.is_reserved_spelling(candidate, package)
        })
    }

    fn is_reserved_spelling(&self, spelling: &str, package: &PackageNames) -> bool {
        is_go_reserved_word(spelling) || self.is_declared_outside_body(spelling, package)
    }

    fn is_declared_outside_body(&self, spelling: &str, package: &PackageNames) -> bool {
        self.declares_type_param(spelling) || package.is_package_block_name(spelling)
    }

    fn is_go_name_taken(&self, go_name: &str, package: &PackageNames) -> bool {
        self.is_reserved_spelling(go_name, package)
            || self.names.issued.contains(go_name)
            || self.has_binding_for_go_name(go_name)
            || self.is_go_name_declared(go_name)
    }
}
