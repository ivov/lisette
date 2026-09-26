use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::go_names::is_go_reserved_word;

use super::{DeclarationKind, DeclarationScope, ScopeState};

#[derive(Default)]
pub(super) struct LocalNames {
    issued: HashSet<String>,
    generated: Vec<GeneratedLocal>,
}

struct GeneratedLocal {
    current: String,
    base: String,
}

impl ScopeState {
    pub(crate) fn fresh_go_name(&mut self, hint: Option<&str>) -> String {
        let base = hint.unwrap_or("tmp").to_string();
        let candidate = self.free_name_from(&base, |name| self.is_go_name_taken(name));
        self.names.issued.insert(candidate.clone());
        self.names.generated.push(GeneratedLocal {
            current: candidate.clone(),
            base,
        });
        candidate
    }

    pub(crate) fn fresh_binding_go_name(&mut self, hint: &str) -> String {
        let candidate = self.free_name_from(hint, |name| self.is_go_name_taken(name));
        self.names.issued.insert(candidate.clone());
        candidate
    }

    fn declares_type_parameter(&self, go_name: &str) -> bool {
        self.frames.iter().any(|frame| match &frame.declarations {
            DeclarationScope::Transparent => false,
            DeclarationScope::Block(names) | DeclarationScope::Function(names) => {
                names.get(go_name) == Some(&DeclarationKind::TypeParameter)
            }
        })
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
    ) -> HashMap<String, String> {
        let generated: HashSet<&str> = self
            .names
            .generated
            .iter()
            .map(|local| local.current.as_str())
            .collect();
        // A source binding that never reached the body frees its name.
        let mut taken: HashSet<String> = HashSet::default();
        taken.extend(
            present
                .iter()
                .filter(|name| !generated.contains(name.as_str()))
                .cloned(),
        );
        let mut settled: HashMap<String, String> = HashMap::default();
        for local in &self.names.generated {
            if !present.contains(&local.current) {
                continue;
            }
            let name = self.free_name_from(&local.base, |candidate| {
                taken.contains(candidate)
                    || is_go_reserved_word(candidate)
                    || self.has_binding_for_go_name(candidate)
                    || self.declares_type_parameter(candidate)
            });
            // Raising a suffix would renumber a name for no reader's benefit.
            if name != local.base || name == local.current {
                taken.insert(local.current.clone());
                continue;
            }
            taken.insert(name.clone());
            settled.insert(local.current.clone(), name);
        }
        settled
    }

    fn is_go_name_taken(&self, go_name: &str) -> bool {
        is_go_reserved_word(go_name)
            || self.names.issued.contains(go_name)
            || self.has_binding_for_go_name(go_name)
            || self.is_go_name_declared(go_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_name_skips_bound_go_name() {
        let mut scope = ScopeState::new();
        scope.bind("value", "tmp");

        assert_eq!(scope.fresh_go_name(None), "tmp_1");
    }

    #[test]
    fn fresh_name_prefers_the_bare_hint_then_numbers_it() {
        let mut scope = ScopeState::new();

        assert_eq!(scope.fresh_go_name(Some("value")), "value");
        assert_eq!(scope.fresh_go_name(Some("value")), "value_1");
        assert_eq!(scope.fresh_go_name(Some("other")), "other");
    }

    #[test]
    fn fresh_name_never_spells_a_go_reserved_word() {
        let mut scope = ScopeState::new();

        assert_eq!(scope.fresh_go_name(Some("range")), "range_1");
        assert_eq!(scope.fresh_go_name(Some("len")), "len_1");
    }
}
