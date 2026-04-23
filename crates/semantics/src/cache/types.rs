use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;
use serde::{Deserialize, Serialize};
use syntax::ast::{
    Annotation, AttributeArg, Generic, Span, StructKind, Visibility as FieldVisibility,
};
use syntax::program::{Definition, Interface, MethodSignatures, Visibility};
use syntax::types::Type;

/// Span stored as file index + byte offsets.
/// file_index refers to position in ModuleInterface.files array (sorted by filename).
/// When loading from cache, file indices are remapped to newly assigned file IDs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedSpan {
    pub file_index: u32,
    pub byte_offset: u32,
    pub byte_length: u32,
}

impl CachedSpan {
    pub fn from_span(span: &Span, file_id_to_index: &HashMap<u32, u32>) -> Self {
        Self {
            file_index: *file_id_to_index.get(&span.file_id).unwrap_or(&0),
            byte_offset: span.byte_offset,
            byte_length: span.byte_length,
        }
    }

    pub fn to_span(&self, file_ids: &[u32]) -> Span {
        Span {
            file_id: file_ids.get(self.file_index as usize).copied().unwrap_or(0),
            byte_offset: self.byte_offset,
            byte_length: self.byte_length,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedGeneric {
    pub name: String,
    pub bounds: Vec<Annotation>,
    pub span: CachedSpan,
}

impl CachedGeneric {
    pub fn from_generic(generic: &Generic, file_id_to_index: &HashMap<u32, u32>) -> Self {
        Self {
            name: generic.name.to_string(),
            bounds: generic.bounds.clone(),
            span: CachedSpan::from_span(&generic.span, file_id_to_index),
        }
    }

    pub fn to_generic(&self, file_ids: &[u32]) -> Generic {
        Generic {
            name: EcoString::from(self.name.as_str()),
            bounds: self.bounds.clone(),
            span: self.span.to_span(file_ids),
        }
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
    pub fn from_literal(lit: &syntax::ast::Literal) -> Self {
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
            Literal::String(v) => CachedLiteral::String(v.clone()),
            Literal::Char(v) => CachedLiteral::Char(v.clone()),
            // These shouldn't appear in ValueEnum variants
            Literal::Imaginary(_) | Literal::FormatString(_) | Literal::Slice(_) => {
                CachedLiteral::Integer {
                    value: 0,
                    text: None,
                }
            }
        }
    }

    pub fn to_literal(&self) -> syntax::ast::Literal {
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
            CachedLiteral::String(v) => Literal::String(v.clone()),
            CachedLiteral::Char(v) => Literal::Char(v.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedAttribute {
    pub name: String,
    pub args: Vec<AttributeArg>,
}

impl CachedAttribute {
    pub fn from_attribute(attribute: &syntax::ast::Attribute) -> Self {
        Self {
            name: attribute.name.clone(),
            args: attribute.args.clone(),
        }
    }

    pub fn to_attribute(&self) -> syntax::ast::Attribute {
        syntax::ast::Attribute {
            name: self.name.clone(),
            args: self.args.clone(),
            span: Span::dummy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedStructField {
    pub name: String,
    pub name_span: CachedSpan,
    pub ty: Type,
    pub visibility: FieldVisibility,
    pub attributes: Vec<CachedAttribute>,
    pub doc: Option<String>,
}

impl CachedStructField {
    pub fn from_field(
        field: &syntax::ast::StructFieldDefinition,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        Self {
            name: field.name.to_string(),
            name_span: CachedSpan::from_span(&field.name_span, file_id_to_index),
            ty: Clone::clone(&field.ty),
            visibility: field.visibility,
            attributes: field
                .attributes
                .iter()
                .map(CachedAttribute::from_attribute)
                .collect(),
            doc: field.doc.clone(),
        }
    }

    pub fn to_field(&self, file_ids: &[u32]) -> syntax::ast::StructFieldDefinition {
        syntax::ast::StructFieldDefinition {
            doc: self.doc.clone(),
            name: self.name.clone().into(),
            name_span: self.name_span.to_span(file_ids),
            ty: self.ty.clone(),
            visibility: self.visibility,
            attributes: self.attributes.iter().map(|a| a.to_attribute()).collect(),
            annotation: Annotation::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedEnumVariant {
    pub name: String,
    pub name_span: CachedSpan,
    pub fields: CachedVariantFields,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedVariantFields {
    Unit,
    Tuple(Vec<CachedEnumField>),
    Struct(Vec<CachedEnumField>),
}

impl CachedVariantFields {
    pub fn from_variant_fields(fields: &syntax::ast::VariantFields) -> Self {
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

    pub fn to_variant_fields(&self) -> syntax::ast::VariantFields {
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
    pub fn from_variant(
        variant: &syntax::ast::EnumVariant,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        Self {
            name: variant.name.to_string(),
            name_span: CachedSpan::from_span(&variant.name_span, file_id_to_index),
            fields: CachedVariantFields::from_variant_fields(&variant.fields),
            doc: variant.doc.clone(),
        }
    }

    pub fn to_variant(&self, file_ids: &[u32]) -> syntax::ast::EnumVariant {
        syntax::ast::EnumVariant {
            doc: self.doc.clone(),
            name: self.name.clone().into(),
            name_span: self.name_span.to_span(file_ids),
            fields: self.fields.to_variant_fields(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedEnumField {
    pub name: String,
    pub ty: Type,
}

impl CachedEnumField {
    pub fn from_field(field: &syntax::ast::EnumFieldDefinition) -> Self {
        Self {
            name: field.name.to_string(),
            ty: Clone::clone(&field.ty),
        }
    }

    pub fn to_field(&self) -> syntax::ast::EnumFieldDefinition {
        syntax::ast::EnumFieldDefinition {
            name: self.name.clone().into(),
            name_span: Span::dummy(),
            ty: self.ty.clone(),
            annotation: Annotation::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedValueEnumVariant {
    pub name: String,
    pub name_span: CachedSpan,
    pub value: CachedLiteral,
    pub doc: Option<String>,
}

impl CachedValueEnumVariant {
    pub fn from_variant(
        variant: &syntax::ast::ValueEnumVariant,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        Self {
            name: variant.name.to_string(),
            name_span: CachedSpan::from_span(&variant.name_span, file_id_to_index),
            value: CachedLiteral::from_literal(&variant.value),
            doc: variant.doc.clone(),
        }
    }

    pub fn to_variant(&self, file_ids: &[u32]) -> syntax::ast::ValueEnumVariant {
        syntax::ast::ValueEnumVariant {
            doc: self.doc.clone(),
            name: self.name.clone().into(),
            name_span: self.name_span.to_span(file_ids),
            value: self.value.to_literal(),
            value_span: Span::dummy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedInterface {
    pub name: String,
    pub generics: Vec<CachedGeneric>,
    pub parents: Vec<Type>,
    pub methods: HashMap<String, Type>,
}

impl CachedInterface {
    pub fn from_interface(iface: &Interface, file_id_to_index: &HashMap<u32, u32>) -> Self {
        Self {
            name: iface.name.to_string(),
            generics: iface
                .generics
                .iter()
                .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                .collect(),
            parents: iface.parents.iter().map(Clone::clone).collect(),
            methods: iface
                .methods
                .iter()
                .map(|(k, v)| (k.to_string(), Clone::clone(v)))
                .collect(),
        }
    }

    pub fn to_interface(&self, file_ids: &[u32]) -> Interface {
        Interface {
            name: EcoString::from(self.name.as_str()),
            generics: self
                .generics
                .iter()
                .map(|g| g.to_generic(file_ids))
                .collect(),
            parents: self.parents.to_vec(),
            methods: self
                .methods
                .iter()
                .map(|(k, v)| (EcoString::from(k.as_str()), v.clone()))
                .collect(),
        }
    }
}

/// Serializable version of Definition. Types are frozen before the cache
/// writer is reached, so `Var` cannot appear.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedDefinition {
    TypeAlias {
        name: String,
        name_span: CachedSpan,
        generics: Vec<CachedGeneric>,
        ty: Type,
        methods: HashMap<String, Type>,
        is_opaque: bool,
        doc: Option<String>,
    },
    Enum {
        name: String,
        name_span: CachedSpan,
        ty: Type,
        generics: Vec<CachedGeneric>,
        variants: Vec<CachedEnumVariant>,
        methods: HashMap<String, Type>,
        doc: Option<String>,
    },
    ValueEnum {
        name: String,
        name_span: CachedSpan,
        ty: Type,
        variants: Vec<CachedValueEnumVariant>,
        underlying_ty: Option<Type>,
        methods: HashMap<String, Type>,
        doc: Option<String>,
    },
    Struct {
        name: String,
        name_span: CachedSpan,
        ty: Type,
        generics: Vec<CachedGeneric>,
        fields: Vec<CachedStructField>,
        kind: StructKind,
        methods: HashMap<String, Type>,
        constructor: Option<Type>,
        doc: Option<String>,
    },
    Interface {
        name_span: CachedSpan,
        ty: Type,
        definition: CachedInterface,
        doc: Option<String>,
    },
    Value {
        name_span: Option<CachedSpan>,
        ty: Type,
        allowed_lints: Vec<String>,
        go_hints: Vec<String>,
        go_name: Option<String>,
        doc: Option<String>,
    },
}

impl CachedDefinition {
    /// Create a CachedDefinition from a Definition.
    /// Only call this for public definitions that should be cached.
    pub fn from_definition(definition: &Definition, file_id_to_index: &HashMap<u32, u32>) -> Self {
        match definition {
            Definition::TypeAlias {
                name,
                name_span,
                generics,
                ty,
                methods,
                annotation,
                doc,
                ..
            } => CachedDefinition::TypeAlias {
                name: name.to_string(),
                name_span: CachedSpan::from_span(name_span, file_id_to_index),
                generics: generics
                    .iter()
                    .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                    .collect(),
                ty: Clone::clone(ty),
                methods: Self::convert_methods(methods),
                is_opaque: annotation.is_opaque(),
                doc: doc.clone(),
            },
            Definition::Enum {
                name,
                name_span,
                ty,
                generics,
                variants,
                methods,
                doc,
                ..
            } => CachedDefinition::Enum {
                name: name.to_string(),
                name_span: CachedSpan::from_span(name_span, file_id_to_index),
                ty: Clone::clone(ty),
                generics: generics
                    .iter()
                    .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                    .collect(),
                variants: variants
                    .iter()
                    .map(|v| CachedEnumVariant::from_variant(v, file_id_to_index))
                    .collect(),
                methods: Self::convert_methods(methods),
                doc: doc.clone(),
            },
            Definition::ValueEnum {
                name,
                name_span,
                ty,
                variants,
                underlying_ty,
                methods,
                doc,
                ..
            } => CachedDefinition::ValueEnum {
                name: name.to_string(),
                name_span: CachedSpan::from_span(name_span, file_id_to_index),
                ty: Clone::clone(ty),
                variants: variants
                    .iter()
                    .map(|v| CachedValueEnumVariant::from_variant(v, file_id_to_index))
                    .collect(),
                underlying_ty: underlying_ty.clone(),
                methods: Self::convert_methods(methods),
                doc: doc.clone(),
            },
            Definition::Struct {
                name,
                name_span,
                ty,
                generics,
                fields,
                kind,
                methods,
                constructor,
                doc,
                ..
            } => CachedDefinition::Struct {
                name: name.to_string(),
                name_span: CachedSpan::from_span(name_span, file_id_to_index),
                ty: Clone::clone(ty),
                generics: generics
                    .iter()
                    .map(|g| CachedGeneric::from_generic(g, file_id_to_index))
                    .collect(),
                fields: fields
                    .iter()
                    .map(|f| CachedStructField::from_field(f, file_id_to_index))
                    .collect(),
                kind: *kind,
                methods: Self::convert_methods(methods),
                constructor: constructor.clone(),
                doc: doc.clone(),
            },
            Definition::Interface {
                ty,
                name_span,
                definition,
                doc,
                ..
            } => CachedDefinition::Interface {
                name_span: CachedSpan::from_span(name_span, file_id_to_index),
                ty: Clone::clone(ty),
                definition: CachedInterface::from_interface(definition, file_id_to_index),
                doc: doc.clone(),
            },
            Definition::Value {
                ty,
                name_span,
                allowed_lints,
                go_hints,
                go_name,
                doc,
                ..
            } => CachedDefinition::Value {
                name_span: name_span.map(|s| CachedSpan::from_span(&s, file_id_to_index)),
                ty: Clone::clone(ty),
                allowed_lints: allowed_lints.clone(),
                go_hints: go_hints.clone(),
                go_name: go_name.clone(),
                doc: doc.clone(),
            },
        }
    }

    fn convert_methods(methods: &MethodSignatures) -> HashMap<String, Type> {
        methods
            .iter()
            .map(|(k, v)| (k.to_string(), Clone::clone(v)))
            .collect()
    }

    fn restore_methods(methods: &HashMap<String, Type>) -> MethodSignatures {
        methods
            .iter()
            .map(|(k, v)| (EcoString::from(k.as_str()), v.clone()))
            .collect()
    }

    pub fn to_definition(&self, file_ids: &[u32]) -> Definition {
        match self {
            CachedDefinition::TypeAlias {
                name,
                name_span,
                generics,
                ty,
                methods,
                is_opaque,
                doc,
            } => Definition::TypeAlias {
                visibility: Visibility::Public,
                name: EcoString::from(name.as_str()),
                name_span: name_span.to_span(file_ids),
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                annotation: if *is_opaque {
                    Annotation::Opaque {
                        span: Span::dummy(),
                    }
                } else {
                    Annotation::Unknown
                },
                ty: ty.clone(),
                methods: Self::restore_methods(methods),
                doc: doc.clone(),
            },
            CachedDefinition::Enum {
                name,
                name_span,
                ty,
                generics,
                variants,
                methods,
                doc,
            } => Definition::Enum {
                visibility: Visibility::Public,
                name: EcoString::from(name.as_str()),
                name_span: name_span.to_span(file_ids),
                ty: ty.clone(),
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                variants: variants.iter().map(|v| v.to_variant(file_ids)).collect(),
                methods: Self::restore_methods(methods),
                doc: doc.clone(),
            },
            CachedDefinition::ValueEnum {
                name,
                name_span,
                ty,
                variants,
                underlying_ty,
                methods,
                doc,
            } => Definition::ValueEnum {
                visibility: Visibility::Public,
                name: EcoString::from(name.as_str()),
                name_span: name_span.to_span(file_ids),
                ty: ty.clone(),
                variants: variants.iter().map(|v| v.to_variant(file_ids)).collect(),
                underlying_ty: underlying_ty.clone(),
                methods: Self::restore_methods(methods),
                doc: doc.clone(),
            },
            CachedDefinition::Struct {
                name,
                name_span,
                ty,
                generics,
                fields,
                kind,
                methods,
                constructor,
                doc,
            } => Definition::Struct {
                visibility: Visibility::Public,
                name: EcoString::from(name.as_str()),
                name_span: name_span.to_span(file_ids),
                ty: ty.clone(),
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                fields: fields.iter().map(|f| f.to_field(file_ids)).collect(),
                kind: *kind,
                methods: Self::restore_methods(methods),
                constructor: constructor.clone(),
                doc: doc.clone(),
            },
            CachedDefinition::Interface {
                name_span,
                ty,
                definition,
                doc,
            } => Definition::Interface {
                visibility: Visibility::Public,
                ty: ty.clone(),
                name_span: name_span.to_span(file_ids),
                definition: definition.to_interface(file_ids),
                doc: doc.clone(),
            },
            CachedDefinition::Value {
                name_span,
                ty,
                allowed_lints,
                go_hints,
                go_name,
                doc,
            } => Definition::Value {
                visibility: Visibility::Public,
                ty: ty.clone(),
                name_span: name_span.as_ref().map(|s| s.to_span(file_ids)),
                allowed_lints: allowed_lints.clone(),
                go_hints: go_hints.clone(),
                go_name: go_name.clone(),
                doc: doc.clone(),
            },
        }
    }
}
