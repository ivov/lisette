use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::ast::{BindingId as AstBindingId, Span};
use crate::types::Symbol;

use super::{Definition, EqualityRecipe, File, FileImport};

/// Name spans of imports that no code in their file uses.
#[derive(Debug, Clone, Default)]
pub struct UnusedImports {
    keys: HashSet<ImportKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportKey(Span);

impl FileImport {
    pub fn key(&self) -> ImportKey {
        ImportKey(self.name_span)
    }
}

impl UnusedImports {
    pub fn mark_unused_import(&mut self, import: &FileImport) {
        self.keys.insert(import.key());
    }

    pub fn is_unused_import(&self, key: ImportKey) -> bool {
        self.keys.contains(&key)
    }

    pub fn merge(&mut self, other: UnusedImports) {
        self.keys.extend(other.keys);
    }
}

#[derive(Debug, Clone, Default)]
pub struct EmitBindings {
    mutations: HashMap<AstBindingId, BindingMutation>,
}

impl EmitBindings {
    pub fn record(&mut self, id: AstBindingId, mutation: Option<BindingMutation>) {
        if let Some(mutation) = mutation {
            self.mutations.insert(id, mutation);
        }
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.mutations
            .get(&id)
            .is_some_and(|mutation| *mutation != BindingMutation::ThroughReference)
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
    synthesized_fields: HashMap<String, Vec<EqualityRecipe>>,
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

    /// Record the equality that the synthesized `equals` of `id` tests each field with, in declaration order.
    pub fn insert_synthesized_fields(&mut self, id: String, fields: Vec<EqualityRecipe>) {
        self.synthesized_fields.insert(id, fields);
    }

    /// The equality that the synthesized `equals` of `id` tests each field with, in declaration order, as the checker selected it.
    pub fn synthesized_fields(&self, id: &str) -> &[EqualityRecipe] {
        self.synthesized_fields
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or_default()
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
    ThroughReference,
    Direct,
    ThroughAlias,
}

impl BindingMutation {
    pub fn merged_with(self, other: Self) -> Self {
        match (self, other) {
            (Self::ThroughAlias, _) | (_, Self::ThroughAlias) => Self::ThroughAlias,
            (Self::Direct, _) | (_, Self::Direct) => Self::Direct,
            (Self::ThroughReference, Self::ThroughReference) => Self::ThroughReference,
        }
    }
}

#[derive(Default)]
pub struct EmitInput {
    pub checked_bodies: super::CheckedBodies,
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub entry_package_id: String,
    pub unused_imports: UnusedImports,
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

    fn import(offset: u32) -> FileImport {
        FileImport {
            name: "go:fmt".into(),
            name_span: span(offset),
            alias: None,
            span: span(offset),
        }
    }

    #[test]
    fn merge_extends_imports() {
        let mut a = UnusedImports::default();
        a.mark_unused_import(&import(1));

        let mut b = UnusedImports::default();
        b.mark_unused_import(&import(3));

        a.merge(b);

        assert!(a.is_unused_import(import(1).key()));
        assert!(a.is_unused_import(import(3).key()));
        assert!(!a.is_unused_import(import(2).key()));
    }
}
