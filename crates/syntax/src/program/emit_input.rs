use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::ast::{BindingId as AstBindingId, Pattern, RestPattern, Span};
use crate::types::Symbol;

use super::{Definition, File};

/// Name spans of unreachable functions and imports (imports keyed by path span).
#[derive(Debug, Clone, Default)]
pub struct UnusedInfo {
    spans: HashSet<Span>,
}

impl UnusedInfo {
    pub fn mark_unused(&mut self, span: Span) {
        self.spans.insert(span);
    }

    pub fn is_unused(&self, span: &Span) -> bool {
        self.spans.contains(span)
    }

    pub fn merge(&mut self, other: UnusedInfo) {
        self.spans.extend(other.spans);
    }
}

#[derive(Debug, Clone, Default)]
pub struct EmitBindings {
    unused: HashSet<AstBindingId>,
    mutations: HashMap<AstBindingId, BindingMutation>,
}

impl EmitBindings {
    pub fn record(&mut self, id: AstBindingId, unused: bool, mutation: Option<BindingMutation>) {
        if unused {
            self.unused.insert(id);
        }
        if let Some(mutation) = mutation {
            self.mutations.insert(id, mutation);
        }
    }

    fn is_unused(&self, id: Option<AstBindingId>) -> bool {
        id.is_some_and(|id| self.unused.contains(&id))
    }

    pub fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        self.is_unused(pattern.binding_id())
    }

    pub fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        self.is_unused(rest.binding_id())
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.mutations.contains_key(&id)
    }

    pub fn is_alias_mutated(&self, id: AstBindingId) -> bool {
        self.mutations.get(&id) == Some(&BindingMutation::ThroughAlias)
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

#[derive(Default)]
pub struct EmitInput {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub entry_package_id: String,
    pub unused: UnusedInfo,
    pub bindings: EmitBindings,
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
    fn merge_extends_spans() {
        let mut a = UnusedInfo::default();
        a.mark_unused(span(1));

        let mut b = UnusedInfo::default();
        b.mark_unused(span(3));

        a.merge(b);

        assert!(a.is_unused(&span(1)));
        assert!(a.is_unused(&span(3)));
    }

    #[test]
    fn unused_lookup_reads_the_binder_id() {
        let pattern = Pattern::AsBinding {
            pattern: Box::new(Pattern::WildCard { span: span(5) }),
            name: "rest".into(),
            span: Span::new(0, 5, 9),
            name_span: Span::new(0, 10, 4),
            binding: Some(AstBindingId::new(1)),
        };
        let unstamped = Pattern::Identifier {
            identifier: "rest".into(),
            span: Span::new(0, 10, 4),
            binding: None,
        };
        let mut bindings = EmitBindings::default();
        bindings.record(AstBindingId::new(1), true, None);
        bindings.record(AstBindingId::new(2), false, None);

        assert!(bindings.is_unused_binding(&pattern));
        assert!(!bindings.is_unused_binding(&unstamped));
    }
}
