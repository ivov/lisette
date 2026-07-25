use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::Span;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleItemId {
    pub name: EcoString,
}

impl ModuleItemId {
    pub fn new(name: &str) -> Self {
        Self { name: name.into() }
    }

    pub fn equals_method(type_name: &str) -> Self {
        Self {
            name: format!("{type_name}#equals").into(),
        }
    }

    pub fn method(method: &str, receiver: &str) -> Self {
        if method == "equals" {
            Self::equals_method(receiver)
        } else {
            Self::new(method)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructFieldId {
    pub type_name: EcoString,
    pub field_name: EcoString,
}

impl StructFieldId {
    pub fn new(type_name: &str, field_name: &str) -> Self {
        Self {
            type_name: type_name.into(),
            field_name: field_name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumVariantId {
    pub enum_name: EcoString,
    pub variant_name: EcoString,
}

impl EnumVariantId {
    pub fn new(enum_name: &str, variant_name: &str) -> Self {
        Self {
            enum_name: enum_name.into(),
            variant_name: variant_name.into(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ReferenceGraph {
    edges: HashMap<ModuleItemId, HashSet<ModuleItemId>>,
    entrypoints: HashSet<ModuleItemId>,
    items: HashMap<ModuleItemId, ItemInfo>,
    unused_struct_fields: HashMap<StructFieldId, Span>,
    unused_enum_variants: HashMap<EnumVariantId, Span>,
}

impl ReferenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_item(&mut self, id: ModuleItemId, span: Span, kind: ItemKind, is_entry_point: bool) {
        self.items.insert(id.clone(), ItemInfo { span, kind });
        if is_entry_point {
            self.entrypoints.insert(id);
        }
    }

    pub fn add_import(&mut self, id: ModuleItemId, span: Span, statement_span: Span) {
        self.items.insert(
            id,
            ItemInfo {
                span,
                kind: ItemKind::Import { statement_span },
            },
        );
    }

    pub fn add_reference(&mut self, from: &ModuleItemId, to: ModuleItemId) {
        self.edges.entry(from.clone()).or_default().insert(to);
    }

    pub fn mark_as_used(&mut self, id: ModuleItemId) {
        self.entrypoints.insert(id);
    }

    pub fn compute_reachable(&self) -> HashSet<ModuleItemId> {
        let mut reachable = HashSet::default();
        let mut worklist: Vec<ModuleItemId> = self.entrypoints.iter().cloned().collect();

        while let Some(item) = worklist.pop() {
            if reachable.contains(&item) {
                continue;
            }
            reachable.insert(item.clone());

            if let Some(refs) = self.edges.get(&item) {
                for referenced in refs {
                    if !reachable.contains(referenced) {
                        worklist.push(referenced.clone());
                    }
                }
            }
        }

        reachable
    }

    pub fn get_unreachable(&self) -> Vec<&ModuleItemId> {
        let reachable = self.compute_reachable();
        self.items
            .keys()
            .filter(|id| !reachable.contains(*id))
            .collect()
    }

    pub fn get_item(&self, id: &ModuleItemId) -> Option<&ItemInfo> {
        self.items.get(id)
    }

    pub fn add_struct_field(&mut self, id: StructFieldId, span: Span) {
        self.unused_struct_fields.insert(id, span);
    }

    pub fn mark_struct_field_used(&mut self, id: StructFieldId) {
        self.unused_struct_fields.remove(&id);
    }

    pub fn add_enum_variant(&mut self, id: EnumVariantId, span: Span) {
        self.unused_enum_variants.insert(id, span);
    }

    pub fn mark_enum_variant_used(&mut self, id: EnumVariantId) {
        self.unused_enum_variants.remove(&id);
    }

    pub fn get_unused_struct_fields(&self) -> impl Iterator<Item = &Span> {
        self.unused_struct_fields.values()
    }

    pub fn get_unused_enum_variants(&self) -> impl Iterator<Item = &Span> {
        self.unused_enum_variants.values()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Import { statement_span: Span },
    Type,
    Function,
    Constant,
}

#[derive(Debug, Clone)]
pub struct ItemInfo {
    pub span: Span,
    pub kind: ItemKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(offset: u32) -> Span {
        Span::new(0, offset, 1)
    }

    #[test]
    fn references_make_registered_items_reachable_without_a_second_node_registry() {
        let mut graph = ReferenceGraph::new();
        let root = ModuleItemId::new("root");
        let child = ModuleItemId::new("child");
        graph.add_item(root.clone(), span(0), ItemKind::Function, true);
        graph.add_item(child.clone(), span(1), ItemKind::Function, false);
        graph.add_reference(&root, child);

        assert!(graph.get_unreachable().is_empty());
    }

    #[test]
    fn using_member_removes_it_from_the_unused_candidates() {
        let mut graph = ReferenceGraph::new();
        let field = StructFieldId::new("module.Type", "field");
        let variant = EnumVariantId::new("Type", "Variant");
        graph.add_struct_field(field.clone(), span(0));
        graph.add_enum_variant(variant.clone(), span(1));

        graph.mark_struct_field_used(field);
        graph.mark_enum_variant_used(variant);

        assert_eq!(
            (
                graph.get_unused_struct_fields().count(),
                graph.get_unused_enum_variants().count(),
            ),
            (0, 0)
        );
    }
}
