use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;

use syntax::ast::Span;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PackageItemId {
    Definition(EcoString),
    EqualityMethod(EcoString),
    Import { file_id: u32, alias: EcoString },
}

impl PackageItemId {
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
        Self::EqualityMethod(type_name.into())
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
    index: HashMap<PackageItemId, NodeId>,
    nodes: Vec<Node>,
    unused_members: HashMap<MemberId, Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeId(u32);

#[derive(Debug)]
struct Node {
    id: PackageItemId,
    info: Option<ItemInfo>,
    edges: Vec<NodeId>,
}

const _: () = assert!(size_of::<NodeId>() == 4);

impl ReferenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    fn node(&mut self, id: &PackageItemId) -> NodeId {
        if let Some(index) = self.index.get(id) {
            return *index;
        }
        let index = NodeId(u32::try_from(self.nodes.len()).expect("too many package items"));
        self.nodes.push(Node {
            id: id.clone(),
            info: None,
            edges: Vec::new(),
        });
        self.index.insert(id.clone(), index);
        index
    }

    pub fn add_item(
        &mut self,
        id: PackageItemId,
        span: Span,
        kind: ItemKind,
        is_entry_point: bool,
    ) {
        let node = self.node(&id);
        self.nodes[node.0 as usize].info = Some(ItemInfo {
            span,
            kind,
            is_entry_point,
        });
    }

    pub fn add_import(&mut self, id: PackageItemId, span: Span, statement_span: Span) {
        self.add_item(id, span, ItemKind::Import { statement_span }, false);
    }

    pub fn add_reference(&mut self, from: &PackageItemId, to: PackageItemId) {
        let from = self.node(from);
        let to = self.node(&to);
        let edges = &mut self.nodes[from.0 as usize].edges;
        if !edges.contains(&to) {
            edges.push(to);
        }
    }

    pub fn mark_as_used(&mut self, id: PackageItemId) {
        if let Some(node) = self.index.get(&id)
            && let Some(item) = &mut self.nodes[node.0 as usize].info
        {
            item.is_entry_point = true;
        }
    }

    pub fn analyze(&self) -> ReferenceUsage<'_> {
        let mut reachable = vec![false; self.nodes.len()];
        let mut worklist = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if node.info.as_ref().is_some_and(|info| info.is_entry_point) {
                reachable[index] = true;
                worklist.push(NodeId(index as u32));
            }
        }
        while let Some(item) = worklist.pop() {
            for referenced in &self.nodes[item.0 as usize].edges {
                if !reachable[referenced.0 as usize] {
                    reachable[referenced.0 as usize] = true;
                    worklist.push(*referenced);
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
    reachable: Vec<bool>,
}

impl<'a> ReferenceUsage<'a> {
    pub fn unreachable_items(&self) -> impl Iterator<Item = (&PackageItemId, &ItemInfo)> {
        self.graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if self.reachable[index] {
                    return None;
                }
                node.info.as_ref().map(|info| (&node.id, info))
            })
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
        let root = PackageItemId::new("root");
        let child = PackageItemId::new("child");
        graph.add_item(root.clone(), span(0), ItemKind::Function, true);
        graph.add_item(child.clone(), span(1), ItemKind::Function, false);
        graph.add_reference(&root, child);

        assert!(graph.analyze().unreachable_items().next().is_none());
    }

    #[test]
    fn using_member_removes_it_from_the_unused_candidates() {
        let mut graph = ReferenceGraph::new();
        let field = StructFieldId::new("package.Type", "field");
        let variant = EnumVariantId::new("Type", "Variant");
        graph.add_struct_field(field.clone(), span(0));
        graph.add_enum_variant(variant.clone(), span(1));

        graph.mark_struct_field_used(field);
        graph.mark_enum_variant_used(variant);

        assert_eq!(graph.unused_members().count(), 0);
    }

    #[test]
    fn equality_method_identity_does_not_collide_with_definition_names() {
        assert_ne!(
            PackageItemId::equals_method("Thing"),
            PackageItemId::new("Thing#equals"),
        );
    }

    #[test]
    fn reachability_follows_early_edges_through_unregistered_nodes_and_cycles() {
        let mut graph = ReferenceGraph::new();
        let root = PackageItemId::new("root");
        let bridge = PackageItemId::new("bridge");
        let child = PackageItemId::new("child");
        let unused = PackageItemId::new("unused");
        graph.add_reference(&root, bridge.clone());
        graph.add_reference(&root, bridge.clone());
        graph.add_reference(&bridge, child.clone());
        graph.add_reference(&child, root.clone());
        graph.add_reference(&unused, unused.clone());
        graph.add_item(root, span(0), ItemKind::Function, true);
        graph.add_item(child, span(1), ItemKind::Function, false);
        graph.add_item(unused.clone(), span(2), ItemKind::Function, false);

        let unreachable: Vec<_> = graph
            .analyze()
            .unreachable_items()
            .map(|(id, _)| id.clone())
            .collect();
        assert_eq!(unreachable, [unused]);
    }

    #[test]
    fn marking_an_unregistered_reference_target_does_not_make_it_an_entry_point() {
        let mut graph = ReferenceGraph::new();
        let item = PackageItemId::new("item");
        graph.add_reference(&PackageItemId::new("caller"), item.clone());
        graph.mark_as_used(item.clone());
        graph.add_item(item.clone(), span(0), ItemKind::Function, false);
        assert_eq!(graph.analyze().unreachable_items().count(), 1);

        graph.mark_as_used(item);
        assert_eq!(graph.analyze().unreachable_items().count(), 0);
    }

    #[test]
    fn reregistering_an_item_replaces_its_span_and_entry_point_status() {
        let mut graph = ReferenceGraph::new();
        let item = PackageItemId::new("item");
        graph.add_item(item.clone(), span(0), ItemKind::Function, true);
        graph.add_item(item.clone(), span(1), ItemKind::Function, false);

        let unreachable: Vec<_> = graph
            .analyze()
            .unreachable_items()
            .map(|(id, info)| (id.clone(), info.span))
            .collect();
        assert_eq!(unreachable, [(item, span(1))]);
    }
}
