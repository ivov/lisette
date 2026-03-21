use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::Span;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleItemId {
    pub module: String,
    pub name: String,
}

impl ModuleItemId {
    pub fn new(module: &str, name: &str) -> Self {
        Self {
            module: module.to_string(),
            name: name.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructFieldId {
    pub type_name: String,
    pub field_name: String,
}

impl StructFieldId {
    pub fn new(type_name: &str, field_name: &str) -> Self {
        Self {
            type_name: type_name.to_string(),
            field_name: field_name.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructFieldInfo {
    pub span: Span,
    pub parent_is_public: bool,
    pub parent_has_serialization_attr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumVariantId {
    pub enum_name: String,
    pub variant_name: String,
}

impl EnumVariantId {
    pub fn new(enum_name: &str, variant_name: &str) -> Self {
        Self {
            enum_name: enum_name.to_string(),
            variant_name: variant_name.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnumVariantInfo {
    pub span: Span,
    pub parent_is_public: bool,
}

#[derive(Debug, Default)]
pub struct ReferenceGraph {
    nodes: HashSet<ModuleItemId>,
    edges: HashMap<ModuleItemId, HashSet<ModuleItemId>>,
    entrypoints: HashSet<ModuleItemId>,
    items: HashMap<ModuleItemId, ItemInfo>,
    struct_fields: HashMap<StructFieldId, StructFieldInfo>,
    used_struct_fields: HashSet<StructFieldId>,
    enum_variants: HashMap<EnumVariantId, EnumVariantInfo>,
    used_enum_variants: HashSet<EnumVariantId>,
}

impl ReferenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_item(&mut self, id: ModuleItemId, span: Span, kind: ItemKind, is_entry_point: bool) {
        self.nodes.insert(id.clone());
        self.items.insert(id.clone(), ItemInfo { span, kind });
        if is_entry_point {
            self.entrypoints.insert(id);
        }
    }

    pub fn add_import(&mut self, id: ModuleItemId, span: Span) {
        self.nodes.insert(id.clone());
        self.items.insert(
            id,
            ItemInfo {
                span,
                kind: ItemKind::Import,
            },
        );
    }

    pub fn add_reference(&mut self, from: &ModuleItemId, to: ModuleItemId) {
        self.edges.entry(from.clone()).or_default().insert(to);
    }

    /// Mark an item as used by adding it to entrypoints.
    /// Used when we know an item is used but it's not reachable through normal call graph.
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
        self.nodes
            .iter()
            .filter(|id| !reachable.contains(*id))
            .collect()
    }

    pub fn get_item(&self, id: &ModuleItemId) -> Option<&ItemInfo> {
        self.items.get(id)
    }

    pub fn add_struct_field(&mut self, id: StructFieldId, info: StructFieldInfo) {
        self.struct_fields.insert(id, info);
    }

    pub fn mark_struct_field_used(&mut self, id: StructFieldId) {
        self.used_struct_fields.insert(id);
    }

    pub fn add_enum_variant(&mut self, id: EnumVariantId, info: EnumVariantInfo) {
        self.enum_variants.insert(id, info);
    }

    pub fn mark_enum_variant_used(&mut self, id: EnumVariantId) {
        self.used_enum_variants.insert(id);
    }

    pub fn get_unused_struct_fields(&self) -> Vec<(&StructFieldId, &StructFieldInfo)> {
        self.struct_fields
            .iter()
            .filter(|(id, info)| {
                !info.parent_is_public
                    && !info.parent_has_serialization_attr
                    && !self.used_struct_fields.contains(*id)
                    && !id.field_name.starts_with('_')
            })
            .collect()
    }

    pub fn get_unused_enum_variants(&self) -> Vec<(&EnumVariantId, &EnumVariantInfo)> {
        self.enum_variants
            .iter()
            .filter(|(id, info)| !info.parent_is_public && !self.used_enum_variants.contains(*id))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Import,
    Type,
    Function,
    Constant,
}

#[derive(Debug, Clone)]
pub struct ItemInfo {
    pub span: Span,
    pub kind: ItemKind,
}
