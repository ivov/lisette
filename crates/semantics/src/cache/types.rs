use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;
use serde::{Deserialize, Serialize};
use syntax::ast::{
    Annotation, AttributeArg, Generic, Span, StructKind, Visibility as FieldVisibility,
};
use syntax::program::{Definition, Interface, MethodSignatures, Visibility};
use syntax::types::{Bound, Type};

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

/// Serializable type representation. Types reach the cache frozen (the
/// `FreezeFolder` pass substitutes every `Type::Var` with its bound type before
/// `analyze()` returns), so `CachedType` has no `Var` variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CachedType {
    Nominal {
        id: String,
        params: Vec<CachedType>,
        underlying_ty: Option<Box<CachedType>>,
    },
    Function {
        params: Vec<CachedType>,
        param_mutability: Vec<bool>,
        bounds: Vec<CachedBound>,
        return_type: Box<CachedType>,
    },
    Forall {
        vars: Vec<String>,
        body: Box<CachedType>,
    },
    Parameter(String),
    Tuple(Vec<CachedType>),
    Never,
    ImportNamespace(String),
    Simple(String),
    Compound {
        kind: String,
        args: Vec<CachedType>,
    },
}

impl CachedType {
    /// Convert a Type to CachedType. Types are frozen before this is reached
    /// (see `FreezeFolder`), so `Type::Var` is unreachable.
    pub fn from_type(ty: &Type) -> Self {
        match ty {
            Type::Var { .. } => {
                unreachable!("cache received an unfrozen Type::Var — freeze pass missed it")
            }
            Type::Nominal {
                id,
                params,
                underlying_ty,
            } => CachedType::Nominal {
                id: id.to_string(),
                params: params.iter().map(CachedType::from_type).collect(),
                underlying_ty: underlying_ty
                    .as_ref()
                    .map(|u| Box::new(CachedType::from_type(u))),
            },
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => CachedType::Function {
                params: params.iter().map(CachedType::from_type).collect(),
                param_mutability: param_mutability.clone(),
                bounds: bounds.iter().map(CachedBound::from_bound).collect(),
                return_type: Box::new(CachedType::from_type(return_type)),
            },
            Type::Forall { vars, body } => CachedType::Forall {
                vars: vars.iter().map(|v| v.to_string()).collect(),
                body: Box::new(CachedType::from_type(body)),
            },
            Type::Parameter(name) => CachedType::Parameter(name.to_string()),
            Type::Tuple(elements) => {
                CachedType::Tuple(elements.iter().map(CachedType::from_type).collect())
            }
            Type::ImportNamespace(module_id) => CachedType::ImportNamespace(module_id.to_string()),
            Type::Simple(kind) => CachedType::Simple(format!("{:?}", kind)),
            Type::Compound { kind, args } => CachedType::Compound {
                kind: format!("{:?}", kind),
                args: args.iter().map(CachedType::from_type).collect(),
            },
            Type::Never | Type::Error | Type::ReceiverPlaceholder => CachedType::Never,
        }
    }

    pub fn to_type(&self) -> Type {
        match self {
            CachedType::Nominal {
                id,
                params,
                underlying_ty,
            } => Type::Nominal {
                id: EcoString::from(id.as_str()),
                params: params.iter().map(|p| p.to_type()).collect(),
                underlying_ty: underlying_ty.as_ref().map(|u| Box::new(u.to_type())),
            },
            CachedType::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| p.to_type()).collect(),
                param_mutability: param_mutability.clone(),
                bounds: bounds.iter().map(|b| b.to_bound()).collect(),
                return_type: Box::new(return_type.to_type()),
            },
            CachedType::Forall { vars, body } => Type::Forall {
                vars: vars.iter().map(|v| EcoString::from(v.as_str())).collect(),
                body: Box::new(body.to_type()),
            },
            CachedType::Parameter(name) => Type::Parameter(EcoString::from(name.as_str())),
            CachedType::Tuple(elements) => {
                Type::Tuple(elements.iter().map(|e| e.to_type()).collect())
            }
            CachedType::Never => Type::Never,
            CachedType::ImportNamespace(module_id) => {
                Type::ImportNamespace(EcoString::from(module_id.as_str()))
            }
            CachedType::Simple(kind_name) => {
                use syntax::types::SimpleKind;
                let kind = match kind_name.as_str() {
                    "Int" => SimpleKind::Int,
                    "Int8" => SimpleKind::Int8,
                    "Int16" => SimpleKind::Int16,
                    "Int32" => SimpleKind::Int32,
                    "Int64" => SimpleKind::Int64,
                    "Uint" => SimpleKind::Uint,
                    "Uint8" => SimpleKind::Uint8,
                    "Uint16" => SimpleKind::Uint16,
                    "Uint32" => SimpleKind::Uint32,
                    "Uint64" => SimpleKind::Uint64,
                    "Uintptr" => SimpleKind::Uintptr,
                    "Byte" => SimpleKind::Byte,
                    "Float32" => SimpleKind::Float32,
                    "Float64" => SimpleKind::Float64,
                    "Complex64" => SimpleKind::Complex64,
                    "Complex128" => SimpleKind::Complex128,
                    "Rune" => SimpleKind::Rune,
                    "Bool" => SimpleKind::Bool,
                    "String" => SimpleKind::String,
                    "Unit" => SimpleKind::Unit,
                    _ => panic!("unknown SimpleKind in cache: {}", kind_name),
                };
                Type::Simple(kind)
            }
            CachedType::Compound { kind, args } => {
                use syntax::types::CompoundKind;
                let kind = match kind.as_str() {
                    "Ref" => CompoundKind::Ref,
                    "Slice" => CompoundKind::Slice,
                    "EnumeratedSlice" => CompoundKind::EnumeratedSlice,
                    "Map" => CompoundKind::Map,
                    "Channel" => CompoundKind::Channel,
                    "Sender" => CompoundKind::Sender,
                    "Receiver" => CompoundKind::Receiver,
                    "VarArgs" => CompoundKind::VarArgs,
                    _ => panic!("unknown CompoundKind in cache: {}", kind),
                };
                Type::Compound {
                    kind,
                    args: args.iter().map(|a| a.to_type()).collect(),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CachedBound {
    pub param_name: String,
    pub generic: CachedType,
    pub ty: CachedType,
}

impl CachedBound {
    pub fn from_bound(bound: &Bound) -> Self {
        Self {
            param_name: bound.param_name.to_string(),
            generic: CachedType::from_type(&bound.generic),
            ty: CachedType::from_type(&bound.ty),
        }
    }

    pub fn to_bound(&self) -> Bound {
        Bound {
            param_name: EcoString::from(self.param_name.as_str()),
            generic: self.generic.to_type(),
            ty: self.ty.to_type(),
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
    pub ty: CachedType,
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
            ty: CachedType::from_type(&field.ty),
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
            ty: self.ty.to_type(),
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
    pub ty: CachedType,
}

impl CachedEnumField {
    pub fn from_field(field: &syntax::ast::EnumFieldDefinition) -> Self {
        Self {
            name: field.name.to_string(),
            ty: CachedType::from_type(&field.ty),
        }
    }

    pub fn to_field(&self) -> syntax::ast::EnumFieldDefinition {
        syntax::ast::EnumFieldDefinition {
            name: self.name.clone().into(),
            name_span: Span::dummy(),
            ty: self.ty.to_type(),
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
    pub parents: Vec<CachedType>,
    pub methods: HashMap<String, CachedType>,
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
            parents: iface.parents.iter().map(CachedType::from_type).collect(),
            methods: iface
                .methods
                .iter()
                .map(|(k, v)| (k.to_string(), CachedType::from_type(v)))
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
            parents: self.parents.iter().map(|p| p.to_type()).collect(),
            methods: self
                .methods
                .iter()
                .map(|(k, v)| (EcoString::from(k.as_str()), v.to_type()))
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
        ty: CachedType,
        methods: HashMap<String, CachedType>,
        is_opaque: bool,
        doc: Option<String>,
    },
    Enum {
        name: String,
        name_span: CachedSpan,
        ty: CachedType,
        generics: Vec<CachedGeneric>,
        variants: Vec<CachedEnumVariant>,
        methods: HashMap<String, CachedType>,
        doc: Option<String>,
    },
    ValueEnum {
        name: String,
        name_span: CachedSpan,
        ty: CachedType,
        variants: Vec<CachedValueEnumVariant>,
        underlying_ty: Option<CachedType>,
        methods: HashMap<String, CachedType>,
        doc: Option<String>,
    },
    Struct {
        name: String,
        name_span: CachedSpan,
        ty: CachedType,
        generics: Vec<CachedGeneric>,
        fields: Vec<CachedStructField>,
        kind: StructKind,
        methods: HashMap<String, CachedType>,
        constructor: Option<CachedType>,
        doc: Option<String>,
    },
    Interface {
        name_span: CachedSpan,
        ty: CachedType,
        definition: CachedInterface,
        doc: Option<String>,
    },
    Value {
        name_span: Option<CachedSpan>,
        ty: CachedType,
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
                ty: CachedType::from_type(ty),
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
                ty: CachedType::from_type(ty),
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
                ty: CachedType::from_type(ty),
                variants: variants
                    .iter()
                    .map(|v| CachedValueEnumVariant::from_variant(v, file_id_to_index))
                    .collect(),
                underlying_ty: underlying_ty.as_ref().map(CachedType::from_type),
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
                ty: CachedType::from_type(ty),
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
                constructor: constructor.as_ref().map(CachedType::from_type),
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
                ty: CachedType::from_type(ty),
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
                ty: CachedType::from_type(ty),
                allowed_lints: allowed_lints.clone(),
                go_hints: go_hints.clone(),
                go_name: go_name.clone(),
                doc: doc.clone(),
            },
        }
    }

    fn convert_methods(methods: &MethodSignatures) -> HashMap<String, CachedType> {
        methods
            .iter()
            .map(|(k, v)| (k.to_string(), CachedType::from_type(v)))
            .collect()
    }

    fn restore_methods(methods: &HashMap<String, CachedType>) -> MethodSignatures {
        methods
            .iter()
            .map(|(k, v)| (EcoString::from(k.as_str()), v.to_type()))
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
                ty: ty.to_type(),
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
                ty: ty.to_type(),
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
                ty: ty.to_type(),
                variants: variants.iter().map(|v| v.to_variant(file_ids)).collect(),
                underlying_ty: underlying_ty.as_ref().map(|u| u.to_type()),
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
                ty: ty.to_type(),
                generics: generics.iter().map(|g| g.to_generic(file_ids)).collect(),
                fields: fields.iter().map(|f| f.to_field(file_ids)).collect(),
                kind: *kind,
                methods: Self::restore_methods(methods),
                constructor: constructor.as_ref().map(|c| c.to_type()),
                doc: doc.clone(),
            },
            CachedDefinition::Interface {
                name_span,
                ty,
                definition,
                doc,
            } => Definition::Interface {
                visibility: Visibility::Public,
                ty: ty.to_type(),
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
                ty: ty.to_type(),
                name_span: name_span.as_ref().map(|s| s.to_span(file_ids)),
                allowed_lints: allowed_lints.clone(),
                go_hints: go_hints.clone(),
                go_name: go_name.clone(),
                doc: doc.clone(),
            },
        }
    }
}
