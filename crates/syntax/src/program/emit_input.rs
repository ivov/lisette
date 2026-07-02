use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{BindingId as AstBindingId, Pattern, RestPattern, Span};
use crate::types::Symbol;

use super::{Definition, File, ModuleInfo};

#[derive(Debug, Clone, Default)]
pub struct UnusedInfo {
    bindings: HashSet<Span>,
    definitions: HashSet<Span>,
    pub imports_by_module: HashMap<EcoString, HashSet<EcoString>>,
}

impl UnusedInfo {
    pub fn mark_binding_unused(&mut self, span: Span) {
        self.bindings.insert(span);
    }

    pub fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier { span, .. } => self.bindings.contains(span),
            Pattern::AsBinding { span, name, .. } => {
                let name_span = Span::new(
                    span.file_id,
                    span.byte_offset + span.byte_length - name.len() as u32,
                    name.len() as u32,
                );
                self.bindings.contains(&name_span)
            }
            _ => false,
        }
    }

    pub fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        match rest {
            RestPattern::Bind { span, .. } => self.bindings.contains(span),
            _ => false,
        }
    }

    pub fn mark_definition_unused(&mut self, span: Span) {
        self.definitions.insert(span);
    }

    pub fn is_unused_definition(&self, span: &Span) -> bool {
        self.definitions.contains(span)
    }

    pub fn merge(&mut self, other: UnusedInfo) {
        let UnusedInfo {
            bindings,
            definitions,
            imports_by_module,
        } = other;
        self.bindings.extend(bindings);
        self.definitions.extend(definitions);
        for (module, imports) in imports_by_module {
            self.imports_by_module
                .entry(module)
                .or_default()
                .extend(imports);
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestFunction {
    pub module_id: String,
    pub qualified_name: String,
    pub title: Option<String>,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct TestIndex {
    tests: Vec<TestFunction>,
}

impl TestIndex {
    pub fn push(&mut self, test: TestFunction) {
        self.tests.push(test);
    }

    pub fn tests(&self) -> &[TestFunction] {
        &self.tests
    }

    pub fn contains_qualified(&self, qualified_name: &str) -> bool {
        self.tests
            .iter()
            .any(|t| t.qualified_name == qualified_name)
    }

    pub fn len(&self) -> usize {
        self.tests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tests.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct EqualityIndex {
    by_id: HashMap<String, EqualityInfo>,
}

#[derive(Debug, Clone)]
pub enum EqualityInfo {
    Method {
        private_to_module: Option<String>,
        synthesized: bool,
    },
    Unusable {
        reason: EqualityUnusableReason,
        private_to_module: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqualityUnusableReason {
    UfcsLowered,
    BoundMismatch,
}

fn visible_from(private_to_module: &Option<String>, current_module: &str) -> bool {
    match private_to_module {
        None => true,
        Some(module) => module == current_module,
    }
}

impl EqualityIndex {
    pub fn insert_method(
        &mut self,
        id: String,
        private_to_module: Option<String>,
        synthesized: bool,
    ) {
        self.by_id.insert(
            id,
            EqualityInfo::Method {
                private_to_module,
                synthesized,
            },
        );
    }

    pub fn insert_unusable(
        &mut self,
        id: String,
        reason: EqualityUnusableReason,
        private_to_module: Option<String>,
    ) {
        self.by_id.insert(
            id,
            EqualityInfo::Unusable {
                reason,
                private_to_module,
            },
        );
    }

    pub fn usable_from(&self, id: &str, current_module: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo::Method { private_to_module, .. })
                if visible_from(private_to_module, current_module)
        )
    }

    pub fn unusable_reason_from(
        &self,
        id: &str,
        current_module: &str,
    ) -> Option<EqualityUnusableReason> {
        match self.by_id.get(id) {
            Some(EqualityInfo::Unusable {
                reason,
                private_to_module,
            }) if visible_from(private_to_module, current_module) => Some(*reason),
            _ => None,
        }
    }

    pub fn is_synthesized(&self, id: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo::Method {
                synthesized: true,
                ..
            })
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct MutationInfo {
    bindings: HashSet<AstBindingId>,
    alias_bindings: HashSet<AstBindingId>,
}

impl MutationInfo {
    pub fn mark_binding_mutated(&mut self, id: AstBindingId) {
        self.bindings.insert(id);
    }

    /// The binding is mutated through an alias, so a call can rebind it.
    pub fn mark_binding_alias_mutated(&mut self, id: AstBindingId) {
        self.bindings.insert(id);
        self.alias_bindings.insert(id);
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.bindings.contains(&id)
    }

    pub fn is_alias_mutated(&self, id: AstBindingId) -> bool {
        self.alias_bindings.contains(&id)
    }
}

pub struct EmitInput {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub const_names: HashSet<Symbol>,
    pub modules: HashMap<String, ModuleInfo>,
    pub entry_module_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub cached_modules: HashSet<String>,
    pub ufcs_methods: HashSet<(String, String)>,
    pub equality_index: EqualityIndex,
    pub test_index: TestIndex,
    pub go_package_names: HashMap<String, String>,
    pub go_module_ids: HashSet<String>,
    pub bound_types: HashMap<crate::ast::Span, crate::types::Type>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(offset: u32) -> Span {
        Span::new(0, offset, 1)
    }

    #[test]
    fn merge_extends_bindings_definitions_and_imports() {
        let mut a = UnusedInfo::default();
        a.mark_binding_unused(span(0));
        a.mark_definition_unused(span(1));
        a.imports_by_module
            .insert("m1".into(), HashSet::from_iter(["x".into()]));

        let mut b = UnusedInfo::default();
        b.mark_binding_unused(span(2));
        b.mark_definition_unused(span(3));
        b.imports_by_module
            .insert("m1".into(), HashSet::from_iter(["y".into()]));
        b.imports_by_module
            .insert("m2".into(), HashSet::from_iter(["z".into()]));

        a.merge(b);

        assert_eq!(a.bindings.len(), 2);
        assert_eq!(a.definitions.len(), 2);
        assert_eq!(a.imports_by_module["m1"].len(), 2);
        assert_eq!(a.imports_by_module["m2"].len(), 1);
    }
}
