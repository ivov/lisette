use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{BindingId as AstBindingId, Pattern, RestPattern, Span};
use crate::types::Symbol;

use super::{Definition, File};

#[derive(Debug, Clone, Default)]
pub struct UnusedInfo {
    symbols: HashSet<Span>,
    pub imports_by_module: HashMap<EcoString, HashSet<EcoString>>,
}

impl UnusedInfo {
    pub fn mark_binding_unused(&mut self, span: Span) {
        self.symbols.insert(span);
    }

    pub fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier { span, .. } => self.symbols.contains(span),
            Pattern::AsBinding { span, name, .. } => {
                let name_span = Span::new(
                    span.file_id,
                    span.byte_offset + span.byte_length - name.len() as u32,
                    name.len() as u32,
                );
                self.symbols.contains(&name_span)
            }
            _ => false,
        }
    }

    pub fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        match rest {
            RestPattern::Bind { span, .. } => self.symbols.contains(span),
            _ => false,
        }
    }

    pub fn mark_definition_unused(&mut self, span: Span) {
        self.symbols.insert(span);
    }

    pub fn is_unused_definition(&self, span: &Span) -> bool {
        self.symbols.contains(span)
    }

    pub fn merge(&mut self, other: UnusedInfo) {
        let UnusedInfo {
            symbols,
            imports_by_module,
        } = other;
        self.symbols.extend(symbols);
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
}

#[derive(Debug, Clone, Default)]
pub struct EqualityIndex {
    by_id: HashMap<String, EqualityInfo>,
}

#[derive(Debug, Clone)]
enum EqualityInfo {
    DeclaredMethod { private_to_module: Option<String> },
    SynthesizedMethod { private_to_module: Option<String> },
    UfcsLowered { private_to_module: Option<String> },
}

fn visible_from(private_to_module: &Option<String>, current_module: &str) -> bool {
    match private_to_module {
        None => true,
        Some(module) => module == current_module,
    }
}

impl EqualityIndex {
    pub fn insert_declared_method(&mut self, id: String, private_to_module: Option<String>) {
        self.by_id
            .insert(id, EqualityInfo::DeclaredMethod { private_to_module });
    }

    pub fn insert_synthesized_method(&mut self, id: String, private_to_module: Option<String>) {
        self.by_id
            .insert(id, EqualityInfo::SynthesizedMethod { private_to_module });
    }

    pub fn insert_ufcs_lowered(&mut self, id: String, private_to_module: Option<String>) {
        self.by_id
            .insert(id, EqualityInfo::UfcsLowered { private_to_module });
    }

    pub fn usable_from(&self, id: &str, current_module: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(
                EqualityInfo::DeclaredMethod { private_to_module }
                    | EqualityInfo::SynthesizedMethod { private_to_module }
            )
                if visible_from(private_to_module, current_module)
        )
    }

    pub fn is_ufcs_lowered_from(&self, id: &str, current_module: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo::UfcsLowered { private_to_module })
                if visible_from(private_to_module, current_module)
        )
    }

    pub fn is_synthesized(&self, id: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo::SynthesizedMethod { .. })
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct MutationInfo {
    bindings: HashMap<AstBindingId, BindingMutation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingMutation {
    Unchanged,
    Direct,
    ThroughAlias,
}

impl BindingMutation {
    pub fn happened(self) -> bool {
        self != Self::Unchanged
    }

    pub fn merged_with(self, other: Self) -> Self {
        match (self, other) {
            (Self::ThroughAlias, _) | (_, Self::ThroughAlias) => Self::ThroughAlias,
            (Self::Direct, _) | (_, Self::Direct) => Self::Direct,
            (Self::Unchanged, Self::Unchanged) => Self::Unchanged,
        }
    }
}

impl MutationInfo {
    pub fn record(&mut self, id: AstBindingId, mutation: BindingMutation) {
        let merged = self.mutation(id).merged_with(mutation);
        if merged.happened() {
            self.bindings.insert(id, merged);
        }
    }

    pub fn mutation(&self, id: AstBindingId) -> BindingMutation {
        self.bindings
            .get(&id)
            .copied()
            .unwrap_or(BindingMutation::Unchanged)
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.mutation(id).happened()
    }

    pub fn is_alias_mutated(&self, id: AstBindingId) -> bool {
        self.mutation(id) == BindingMutation::ThroughAlias
    }
}

pub struct EmitInput {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub entry_module_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub cached_modules: HashSet<String>,
    pub ufcs_methods: HashSet<(String, String)>,
    pub equality_index: EqualityIndex,
    pub test_index: TestIndex,
    pub go_package_names: HashMap<String, String>,
    pub go_module_ids: HashSet<String>,
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

        assert_eq!(a.symbols.len(), 4);
        assert_eq!(a.imports_by_module["m1"].len(), 2);
        assert_eq!(a.imports_by_module["m2"].len(), 1);
    }
}
