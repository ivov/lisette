use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::Span;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModuleItemId {
    Definition(EcoString),
    Import { file_id: u32, alias: EcoString },
}

impl ModuleItemId {
    pub fn new(name: &str) -> Self {
        Self::Definition(name.into())
    }

    pub fn import(file_id: u32, name: &str) -> Self {
        Self::Import {
            file_id,
            alias: name.into(),
        }
    }

    pub fn equals_method(type_name: &str) -> Self {
        Self::Definition(format!("{type_name}#equals").into())
    }

    fn import_alias(&self) -> Option<&str> {
        match self {
            Self::Import { alias, .. } => Some(alias),
            Self::Definition(_) => None,
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
    items: HashMap<ModuleItemId, ItemInfo>,
    unused_members: HashMap<MemberId, Span>,
}

impl ReferenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_item(&mut self, id: ModuleItemId, span: Span, kind: ItemKind, is_entry_point: bool) {
        self.items.insert(
            id,
            ItemInfo {
                span,
                kind,
                is_entry_point,
            },
        );
    }

    pub fn add_import(&mut self, id: ModuleItemId, span: Span, statement_span: Span) {
        self.items.insert(
            id,
            ItemInfo {
                span,
                kind: ItemKind::Import { statement_span },
                is_entry_point: false,
            },
        );
    }

    pub fn add_reference(&mut self, from: &ModuleItemId, to: ModuleItemId) {
        self.edges.entry(from.clone()).or_default().insert(to);
    }

    pub fn mark_as_used(&mut self, id: ModuleItemId) {
        if let Some(item) = self.items.get_mut(&id) {
            item.is_entry_point = true;
        }
    }

    pub fn analyze(&self) -> ReferenceUsage<'_> {
        let mut reachable = HashSet::default();
        let mut worklist: Vec<ModuleItemId> = self
            .items
            .iter()
            .filter(|(_, item)| item.is_entry_point)
            .map(|(id, _)| id.clone())
            .collect();

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

        ReferenceUsage {
            graph: self,
            reachable,
        }
    }

    pub fn add_struct_field(&mut self, id: StructFieldId, span: Span) {
        self.unused_members.insert(MemberId::StructField(id), span);
    }

    pub fn mark_struct_field_used(&mut self, id: StructFieldId) {
        self.unused_members.remove(&MemberId::StructField(id));
    }

    pub fn add_enum_variant(&mut self, id: EnumVariantId, span: Span) {
        self.unused_members.insert(MemberId::EnumVariant(id), span);
    }

    pub fn mark_enum_variant_used(&mut self, id: EnumVariantId) {
        self.unused_members.remove(&MemberId::EnumVariant(id));
    }

    pub fn unused_members(&self) -> impl Iterator<Item = (MemberKind, &Span)> {
        self.unused_members
            .iter()
            .map(|(id, span)| (id.kind(), span))
    }
}

pub struct ReferenceUsage<'a> {
    graph: &'a ReferenceGraph,
    reachable: HashSet<ModuleItemId>,
}

impl<'a> ReferenceUsage<'a> {
    pub fn unreachable_items(&self) -> impl Iterator<Item = (&ModuleItemId, &ItemInfo)> {
        self.graph
            .items
            .iter()
            .filter(|(id, _)| !self.reachable.contains(*id))
    }

    pub fn unused_import_aliases(&self) -> HashSet<String> {
        let mut aliases = HashMap::default();
        for (id, item) in &self.graph.items {
            if let Some(alias) = id.import_alias() {
                debug_assert!(matches!(item.kind, ItemKind::Import { .. }));
                aliases
                    .entry(alias)
                    .and_modify(|all_unused| *all_unused &= !self.reachable.contains(id))
                    .or_insert_with(|| !self.reachable.contains(id));
            }
        }
        aliases
            .into_iter()
            .filter(|(_, all_unused)| *all_unused)
            .map(|(alias, _)| alias.to_string())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MemberId {
    StructField(StructFieldId),
    EnumVariant(EnumVariantId),
}

impl MemberId {
    fn kind(&self) -> MemberKind {
        match self {
            Self::StructField(_) => MemberKind::StructField,
            Self::EnumVariant(_) => MemberKind::EnumVariant,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    StructField,
    EnumVariant,
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
    is_entry_point: bool,
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

        assert!(graph.analyze().unreachable_items().next().is_none());
    }

    #[test]
    fn import_alias_is_unused_only_when_unused_in_every_file() {
        let mut graph = ReferenceGraph::new();
        let used = ModuleItemId::import(0, "dep");
        let unused = ModuleItemId::import(1, "dep");
        graph.add_import(used.clone(), span(0), span(0));
        graph.add_import(unused, span(1), span(1));
        graph.mark_as_used(used);

        assert!(graph.analyze().unused_import_aliases().is_empty());
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

        assert_eq!(graph.unused_members().count(), 0);
    }
}
