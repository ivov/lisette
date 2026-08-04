use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{BindingId as AstBindingId, Pattern, RestPattern, Span};
use crate::types::Symbol;

use super::{Definition, File};

#[derive(Debug, Clone, Default)]
pub struct UnusedInfo {
    symbols: HashSet<Span>,
    pub imports_by_package: HashMap<EcoString, HashSet<EcoString>>,
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
            imports_by_package,
        } = other;
        self.symbols.extend(symbols);
        for (package, imports) in imports_by_package {
            self.imports_by_package
                .entry(package)
                .or_default()
                .extend(imports);
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestFunction {
    qualified_name: Symbol,
    pub title: Option<String>,
    pub doc: Option<String>,
    pub span: Span,
}

impl TestFunction {
    pub fn new(
        package_id: &str,
        name: &str,
        title: Option<String>,
        doc: Option<String>,
        span: Span,
    ) -> Self {
        Self {
            qualified_name: Symbol::from_parts(package_id, name),
            title,
            doc,
            span,
        }
    }

    pub fn package_id(&self) -> &str {
        self.qualified_name
            .without_last_segment()
            .expect("test names are constructed with a package")
    }

    pub fn qualified_name(&self) -> &str {
        self.qualified_name.as_str()
    }

    pub fn name(&self) -> &str {
        self.qualified_name.last_segment()
    }
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
            .any(|test| test.qualified_name == qualified_name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EqualityIndex {
    by_id: HashMap<String, EqualityInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EqualityKind {
    DeclaredMethod,
    SynthesizedMethod,
    UfcsLowered,
}

#[derive(Debug, Clone)]
struct EqualityInfo {
    kind: EqualityKind,
    private_to_package: Option<String>,
}

fn visible_from(private_to_package: &Option<String>, current_package: &str) -> bool {
    match private_to_package {
        None => true,
        Some(package) => package == current_package,
    }
}

impl EqualityIndex {
    pub fn insert_declared_method(&mut self, id: String, private_to_package: Option<String>) {
        self.by_id.insert(
            id,
            EqualityInfo {
                kind: EqualityKind::DeclaredMethod,
                private_to_package,
            },
        );
    }

    pub fn insert_synthesized_method(&mut self, id: String, private_to_package: Option<String>) {
        self.by_id.insert(
            id,
            EqualityInfo {
                kind: EqualityKind::SynthesizedMethod,
                private_to_package,
            },
        );
    }

    pub fn insert_ufcs_lowered(&mut self, id: String, private_to_package: Option<String>) {
        self.by_id.insert(
            id,
            EqualityInfo {
                kind: EqualityKind::UfcsLowered,
                private_to_package,
            },
        );
    }

    pub fn usable_from(&self, id: &str, current_package: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo {
                kind: EqualityKind::DeclaredMethod | EqualityKind::SynthesizedMethod,
                private_to_package,
            }) if visible_from(private_to_package, current_package)
        )
    }

    pub fn is_ufcs_lowered_from(&self, id: &str, current_package: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo {
                kind: EqualityKind::UfcsLowered,
                private_to_package,
            }) if visible_from(private_to_package, current_package)
        )
    }

    pub fn is_synthesized(&self, id: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(EqualityInfo {
                kind: EqualityKind::SynthesizedMethod,
                ..
            })
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct MutationInfo {
    bindings: HashMap<AstBindingId, BindingMutation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingMutation {
    Direct,
    ThroughAlias,
}

impl BindingMutation {
    pub fn merged_with(self, other: Self) -> Self {
        match (self, other) {
            (Self::ThroughAlias, _) | (_, Self::ThroughAlias) => Self::ThroughAlias,
            (Self::Direct, Self::Direct) => Self::Direct,
        }
    }
}

impl MutationInfo {
    pub fn record(&mut self, id: AstBindingId, mutation: BindingMutation) {
        self.bindings
            .entry(id)
            .and_modify(|current| *current = current.merged_with(mutation))
            .or_insert(mutation);
    }

    pub fn mutation(&self, id: AstBindingId) -> Option<BindingMutation> {
        self.bindings.get(&id).copied()
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.bindings.contains_key(&id)
    }

    pub fn is_alias_mutated(&self, id: AstBindingId) -> bool {
        self.mutation(id) == Some(BindingMutation::ThroughAlias)
    }
}

#[derive(Default)]
pub struct EmitInput {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub entry_package_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub cached_packages: HashSet<String>,
    pub equality_index: EqualityIndex,
    pub test_index: TestIndex,
    pub go_package_names: HashMap<String, String>,
    pub go_package_ids: HashSet<String>,
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
        a.imports_by_package
            .insert("m1".into(), HashSet::from_iter(["x".into()]));

        let mut b = UnusedInfo::default();
        b.mark_binding_unused(span(2));
        b.mark_definition_unused(span(3));
        b.imports_by_package
            .insert("m1".into(), HashSet::from_iter(["y".into()]));
        b.imports_by_package
            .insert("m2".into(), HashSet::from_iter(["z".into()]));

        a.merge(b);

        assert_eq!(a.symbols.len(), 4);
        assert_eq!(a.imports_by_package["m1"].len(), 2);
        assert_eq!(a.imports_by_package["m2"].len(), 1);
    }
}
