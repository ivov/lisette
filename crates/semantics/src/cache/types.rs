use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;
use serde::{Deserialize, Serialize};
use syntax::ast::{
    Annotation, AttributeArg, Generic, Span, StructKind, Visibility as FieldVisibility,
};
use syntax::program::{
    AliasKind, Attributes, Definition, DefinitionBody, Interface, MethodSignatures, Module,
    ValueKind, Visibility,
};
use syntax::types::{Symbol, Type};

/// Span stored as file index + byte offsets.
/// file_index refers to position in ModuleInterface.files array (sorted by filename).
/// When loading from cache, file indices are remapped to newly assigned file IDs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedSpan {
    file_index: u32,
    byte_offset: u32,
    byte_length: u32,
}

impl CachedSpan {
    fn from_span(span: &Span, file_id_to_index: &HashMap<u32, u32>) -> Self {
        Self {
            file_index: *file_id_to_index.get(&span.file_id).unwrap_or(&0),
            byte_offset: span.byte_offset,
            byte_length: span.byte_length,
        }
    }

    fn to_span(&self, file_ids: &[u32]) -> Span {
        Span {
            file_id: file_ids.get(self.file_index as usize).copied().unwrap_or(0),
            byte_offset: self.byte_offset,
            byte_length: self.byte_length,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedGeneric {
    name: EcoString,
    bounds: Vec<Annotation>,
    span: CachedSpan,
}

impl CachedGeneric {
    fn from_generic(generic: &Generic, file_id_to_index: &HashMap<u32, u32>) -> Self {
        Self {
            name: generic.name.clone(),
            bounds: generic.bounds().cloned().collect(),
            span: CachedSpan::from_span(&generic.span, file_id_to_index),
        }
    }

    fn to_generic(&self, file_ids: &[u32]) -> Generic {
        Generic::new(
            self.name.clone(),
            self.bounds.clone(),
            self.span.to_span(file_ids),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedLiteral {
    Integer { value: u64, text: Option<String> },
    Float { value: f64, text: Option<String> },
    Boolean(bool),
    String(String),
    Char(String),
}

impl CachedLiteral {
    fn from_literal(lit: &syntax::ast::Literal) -> Self {
        use syntax::ast::Literal;
        match lit {
            Literal::Integer { value, text } => CachedLiteral::Integer {
                value: *value,
                text: text.clone(),
            },
            Literal::Float { value, text } => CachedLiteral::Float {
                value: *value,
                text: text.clone(),
            },
            Literal::Boolean(v) => CachedLiteral::Boolean(*v),
            Literal::String { value, raw } => {
                assert!(
                    !raw,
                    "cached const literals are canonicalized to non-raw strings"
                );
                CachedLiteral::String(value.clone())
            }
            Literal::Char(v) => CachedLiteral::Char(v.clone()),
            // Canonical const literals are never one of these kinds.
            Literal::Imaginary(_) | Literal::FormatString(_) | Literal::Slice(_) => {
                unreachable!("only canonical const literals can be cached")
            }
        }
    }

    fn to_literal(&self) -> syntax::ast::Literal {
        use syntax::ast::Literal;
        match self {
            CachedLiteral::Integer { value, text } => Literal::Integer {
                value: *value,
                text: text.clone(),
            },
            CachedLiteral::Float { value, text } => Literal::Float {
                value: *value,
                text: text.clone(),
            },
            CachedLiteral::Boolean(v) => Literal::Boolean(*v),
            CachedLiteral::String(v) => Literal::String {
                value: v.clone(),
                raw: false,
            },
            CachedLiteral::Char(v) => Literal::Char(v.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedAttribute {
    name: String,
    args: Vec<AttributeArg>,
}

impl CachedAttribute {
    fn from_attribute(attribute: &syntax::ast::Attribute) -> Self {
        Self {
            name: attribute.name.clone(),
            args: attribute.args.clone(),
        }
    }

    fn to_attribute(&self) -> syntax::ast::Attribute {
        syntax::ast::Attribute {
            name: self.name.clone(),
            args: self.args.clone(),
            span: Span::dummy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedStructField {
    name: EcoString,
    name_span: CachedSpan,
    ty: Type,
    visibility: FieldVisibility,
    attributes: Vec<CachedAttribute>,
    doc: Option<String>,
    embedded: bool,
}

impl CachedStructField {
    fn from_field(
        field: &syntax::ast::StructFieldDefinition,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        Self {
            name: field.name.clone(),
            name_span: CachedSpan::from_span(&field.name_span, file_id_to_index),
            ty: Clone::clone(&field.ty),
            visibility: field.visibility,
            attributes: field
                .attributes
                .iter()
                .map(CachedAttribute::from_attribute)
                .collect(),
            doc: field.doc.clone(),
            embedded: field.embedded,
        }
    }

    fn to_field(&self, file_ids: &[u32]) -> syntax::ast::StructFieldDefinition {
        syntax::ast::StructFieldDefinition {
            doc: self.doc.clone(),
            name: self.name.clone(),
            name_span: self.name_span.to_span(file_ids),
            ty: self.ty.clone(),
            visibility: self.visibility,
            attributes: self.attributes.iter().map(|a| a.to_attribute()).collect(),
            annotation: Annotation::Unknown,
            embedded: self.embedded,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedEnumVariant {
    name: EcoString,
    name_span: CachedSpan,
    fields: CachedVariantFields,
    doc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedVariantFields {
    Unit,
    Tuple(Vec<CachedEnumField>),
    Struct(Vec<CachedEnumField>),
}

impl CachedVariantFields {
    fn from_variant_fields(fields: &syntax::ast::VariantFields) -> Self {
        match fields {
            syntax::ast::VariantFields::Unit => CachedVariantFields::Unit,
            syntax::ast::VariantFields::Tuple(fs) => {
                CachedVariantFields::Tuple(fs.iter().map(CachedEnumField::from_field).collect())
            }
            syntax::ast::VariantFields::Struct(fs) => {
                CachedVariantFields::Struct(fs.iter().map(CachedEnumField::from_field).collect())
            }
        }
    }

    fn to_variant_fields(&self) -> syntax::ast::VariantFields {
        match self {
            CachedVariantFields::Unit => syntax::ast::VariantFields::Unit,
            CachedVariantFields::Tuple(fs) => {
                syntax::ast::VariantFields::Tuple(fs.iter().map(|f| f.to_field()).collect())
            }
            CachedVariantFields::Struct(fs) => {
                syntax::ast::VariantFields::Struct(fs.iter().map(|f| f.to_field()).collect())
            }
        }
    }
}

impl CachedEnumVariant {
    fn from_variant(
        variant: &syntax::ast::EnumVariant,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        Self {
            name: variant.name.clone(),
            name_span: CachedSpan::from_span(&variant.name_span, file_id_to_index),
            fields: CachedVariantFields::from_variant_fields(&variant.fields),
            doc: variant.doc.clone(),
        }
    }

    fn to_variant(&self, file_ids: &[u32]) -> syntax::ast::EnumVariant {
        syntax::ast::EnumVariant {
            doc: self.doc.clone(),
            name: self.name.clone(),
            name_span: self.name_span.to_span(file_ids),
            fields: self.fields.to_variant_fields(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedEnumField {
    name: EcoString,
    ty: Type,
}

impl CachedEnumField {
    fn from_field(field: &syntax::ast::EnumFieldDefinition) -> Self {
        Self {
            name: field.name.clone(),
            ty: Clone::clone(&field.ty),
        }
    }

    fn to_field(&self) -> syntax::ast::EnumFieldDefinition {
        syntax::ast::EnumFieldDefinition {
            name: self.name.clone(),
            name_span: Span::dummy(),
            ty: self.ty.clone(),
            annotation: Annotation::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedInterface {
    name: EcoString,
    generics: Vec<CachedGeneric>,
    parents: Vec<Type>,
    pub methods: MethodSignatures,
}

impl CachedInterface {
    fn from_interface(iface: &Interface, file_id_to_index: &HashMap<u32, u32>) -> Self {
        Self {
            name: iface.name.clone(),
            generics: iface
                .generics
                .iter()
                .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                .collect(),
            parents: iface.parents.iter().map(Clone::clone).collect(),
            methods: iface.methods.clone(),
        }
    }

    fn to_interface(&self, file_ids: &[u32]) -> Interface {
        Interface {
            name: self.name.clone(),
            generics: self
                .generics
                .iter()
                .map(|g| g.to_generic(file_ids))
                .collect(),
            parents: self.parents.to_vec(),
            methods: self.methods.clone(),
        }
    }
}

/// Serializable version of Definition. Types are frozen before the cache
/// writer is reached, so `Var` cannot appear. Mirrors the in-memory
/// `Definition` shape: common fields up top, variant-specific data in `body`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedDefinition {
    ty: Type,
    name: Option<EcoString>,
    name_span: Option<CachedSpan>,
    doc: Option<String>,
    pub body: CachedDefinitionBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedValueKind {
    Runtime,
    Constant { value: Option<CachedLiteral> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedDefinitionBody {
    TypeAlias {
        generics: Vec<CachedGeneric>,
        methods: MethodSignatures,
        alias: CachedAliasKind,
        attributes: Attributes,
    },
    Enum {
        generics: Vec<CachedGeneric>,
        variants: Vec<CachedEnumVariant>,
        methods: MethodSignatures,
        attributes: Attributes,
    },
    Struct {
        generics: Vec<CachedGeneric>,
        fields: Vec<CachedStructField>,
        kind: StructKind,
        methods: MethodSignatures,
        constructor: Option<Type>,
        attributes: Attributes,
    },
    Interface {
        definition: CachedInterface,
    },
    Value {
        kind: CachedValueKind,
        allowed_lints: Vec<String>,
        go_hints: Vec<String>,
        go_name: Option<String>,
        go_type_param_recipe: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedAliasKind {
    Opaque,
    Transparent(Type),
}

impl CachedDefinition {
    /// Create a CachedDefinition from a Definition.
    /// Only call this for public definitions that should be cached.
    pub(crate) fn from_definition(
        definition: &Definition,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        let Definition {
            ty,
            name,
            name_span,
            doc,
            body,
            ..
        } = definition;
        let body = match body {
            DefinitionBody::TypeAlias {
                generics,
                alias,
                methods,
                attributes,
            } => CachedDefinitionBody::TypeAlias {
                generics: generics
                    .iter()
                    .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                    .collect(),
                methods: methods.clone(),
                alias: match alias {
                    AliasKind::Opaque(_) => CachedAliasKind::Opaque,
                    AliasKind::Transparent { target, .. } => {
                        CachedAliasKind::Transparent(target.clone())
                    }
                },
                attributes: attributes.clone(),
            },
            DefinitionBody::Enum {
                generics,
                variants,
                methods,
                attributes,
            } => CachedDefinitionBody::Enum {
                generics: generics
                    .iter()
                    .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                    .collect(),
                variants: variants
                    .iter()
                    .map(|v| CachedEnumVariant::from_variant(v, file_id_to_index))
                    .collect(),
                methods: methods.clone(),
                attributes: attributes.clone(),
            },
            DefinitionBody::Struct {
                generics,
                fields,
                kind,
                methods,
                constructor,
                attributes,
            } => CachedDefinitionBody::Struct {
                generics: generics
                    .iter()
                    .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                    .collect(),
                fields: fields
                    .iter()
                    .map(|f| CachedStructField::from_field(f, file_id_to_index))
                    .collect(),
                kind: *kind,
                methods: methods.clone(),
                constructor: constructor.clone(),
                attributes: attributes.clone(),
            },
            DefinitionBody::Interface { definition } => CachedDefinitionBody::Interface {
                definition: CachedInterface::from_interface(definition, file_id_to_index),
            },
            DefinitionBody::Value {
                kind,
                allowed_lints,
                go_hints,
                go_name,
                go_type_param_recipe,
            } => CachedDefinitionBody::Value {
                kind: match kind {
                    ValueKind::Runtime => CachedValueKind::Runtime,
                    ValueKind::Constant { value } => CachedValueKind::Constant {
                        value: value.as_ref().map(CachedLiteral::from_literal),
                    },
                },
                allowed_lints: allowed_lints.clone(),
                go_hints: go_hints.clone(),
                go_name: go_name.clone(),
                go_type_param_recipe: go_type_param_recipe.clone(),
            },
        };
        CachedDefinition {
            ty: ty.clone(),
            name: name.clone(),
            name_span: name_span.map(|s| CachedSpan::from_span(&s, file_id_to_index)),
            doc: doc.clone(),
            body,
        }
    }

    pub(crate) fn install_into(
        &self,
        module: &mut Module,
        qualified_name: Symbol,
        file_ids: &[u32],
    ) {
        let definition = self.to_definition(file_ids);
        module.definitions.insert(qualified_name, definition);
    }

    pub(crate) fn to_definition(&self, file_ids: &[u32]) -> Definition {
        let body = match &self.body {
            CachedDefinitionBody::TypeAlias {
                generics,
                methods,
                alias,
                attributes,
            } => DefinitionBody::TypeAlias {
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                alias: match alias {
                    CachedAliasKind::Opaque => AliasKind::Opaque(Annotation::Opaque {
                        span: Span::dummy(),
                    }),
                    CachedAliasKind::Transparent(target) => AliasKind::Transparent {
                        annotation: Annotation::Unknown,
                        target: target.clone(),
                    },
                },
                methods: methods.clone(),
                attributes: attributes.clone(),
            },
            CachedDefinitionBody::Enum {
                generics,
                variants,
                methods,
                attributes,
            } => DefinitionBody::Enum {
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                variants: variants.iter().map(|v| v.to_variant(file_ids)).collect(),
                methods: methods.clone(),
                attributes: attributes.clone(),
            },
            CachedDefinitionBody::Struct {
                generics,
                fields,
                kind,
                methods,
                constructor,
                attributes,
            } => DefinitionBody::Struct {
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                fields: fields.iter().map(|f| f.to_field(file_ids)).collect(),
                kind: *kind,
                methods: methods.clone(),
                constructor: constructor.clone(),
                attributes: attributes.clone(),
            },
            CachedDefinitionBody::Interface { definition } => DefinitionBody::Interface {
                definition: definition.to_interface(file_ids),
            },
            CachedDefinitionBody::Value {
                kind,
                allowed_lints,
                go_hints,
                go_name,
                go_type_param_recipe,
            } => DefinitionBody::Value {
                kind: match kind {
                    CachedValueKind::Runtime => ValueKind::Runtime,
                    CachedValueKind::Constant { value } => ValueKind::Constant {
                        value: value.as_ref().map(CachedLiteral::to_literal),
                    },
                },
                allowed_lints: allowed_lints.clone(),
                go_hints: go_hints.clone(),
                go_name: go_name.clone(),
                go_type_param_recipe: go_type_param_recipe.clone(),
            },
        };
        Definition {
            visibility: Visibility::Public,
            ty: self.ty.clone(),
            name: self.name.clone(),
            name_span: self.name_span.as_ref().map(|s| s.to_span(file_ids)),
            doc: self.doc.clone(),
            body,
        }
    }
}
