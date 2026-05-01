use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;

use crate::ast::{
    Annotation, EnumVariant, Generic, Span, StructFieldDefinition, StructKind, ValueEnumVariant,
};
use crate::types::Type;

#[derive(Debug, Clone)]
pub enum Definition {
    TypeAlias {
        visibility: Visibility,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        annotation: Annotation,
        ty: Type,
        methods: MethodSignatures,
        doc: Option<String>,
    },
    Enum {
        visibility: Visibility,
        ty: Type,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        variants: Vec<EnumVariant>,
        methods: MethodSignatures,
        doc: Option<String>,
    },
    ValueEnum {
        visibility: Visibility,
        ty: Type,
        name: EcoString,
        name_span: Span,
        variants: Vec<ValueEnumVariant>,
        underlying_ty: Option<Type>,
        methods: MethodSignatures,
        doc: Option<String>,
    },
    Struct {
        visibility: Visibility,
        ty: Type,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        fields: Vec<StructFieldDefinition>,
        kind: StructKind,
        methods: MethodSignatures,
        constructor: Option<Type>,
        doc: Option<String>,
    },
    Interface {
        visibility: Visibility,
        ty: Type,
        name_span: Span,
        definition: Interface,
        doc: Option<String>,
    },
    Value {
        visibility: Visibility,
        ty: Type,
        name_span: Option<Span>,
        allowed_lints: Vec<String>,
        go_hints: Vec<String>,
        go_name: Option<String>,
        doc: Option<String>,
    },
}

impl Definition {
    pub fn ty(&self) -> &Type {
        match self {
            Definition::TypeAlias { ty, .. } => ty,
            Definition::Enum { ty, .. } => ty,
            Definition::ValueEnum { ty, .. } => ty,
            Definition::Struct { ty, .. } => ty,
            Definition::Interface { ty, .. } => ty,
            Definition::Value { ty, .. } => ty,
        }
    }

    pub fn visibility(&self) -> &Visibility {
        match self {
            Definition::TypeAlias { visibility, .. } => visibility,
            Definition::Enum { visibility, .. } => visibility,
            Definition::ValueEnum { visibility, .. } => visibility,
            Definition::Struct { visibility, .. } => visibility,
            Definition::Interface { visibility, .. } => visibility,
            Definition::Value { visibility, .. } => visibility,
        }
    }

    /// A newtype is a single-field, non-generic tuple struct. Relevant
    /// because Go compiles newtypes to named scalar types, so `.0` is a cast
    /// rather than a field access — it cannot be assigned to, and taking
    /// its address is invalid.
    pub fn is_newtype(&self) -> bool {
        matches!(
            self,
            Definition::Struct {
                kind: StructKind::Tuple,
                fields,
                generics,
                ..
            } if fields.len() == 1 && generics.is_empty()
        )
    }

    pub fn allowed_lints(&self) -> &[String] {
        match self {
            Definition::Value { allowed_lints, .. } => allowed_lints,
            _ => &[],
        }
    }

    pub fn go_hints(&self) -> &[String] {
        match self {
            Definition::Value { go_hints, .. } => go_hints,
            _ => &[],
        }
    }

    pub fn go_name(&self) -> Option<&str> {
        match self {
            Definition::Value { go_name, .. } => go_name.as_deref(),
            _ => None,
        }
    }

    pub fn methods_mut(&mut self) -> Option<&mut MethodSignatures> {
        match self {
            Definition::Struct { methods, .. } => Some(methods),
            Definition::TypeAlias { methods, .. } => Some(methods),
            Definition::Enum { methods, .. } => Some(methods),
            Definition::ValueEnum { methods, .. } => Some(methods),
            _ => None,
        }
    }

    pub fn is_type_definition(&self) -> bool {
        matches!(
            self,
            Definition::Struct { .. }
                | Definition::Enum { .. }
                | Definition::ValueEnum { .. }
                | Definition::TypeAlias { .. }
        )
    }

    pub fn is_type_alias(&self) -> bool {
        matches!(self, Definition::TypeAlias { .. })
    }

    pub fn name_span(&self) -> Option<Span> {
        match self {
            Definition::TypeAlias { name_span, .. } => Some(*name_span),
            Definition::Enum { name_span, .. } => Some(*name_span),
            Definition::ValueEnum { name_span, .. } => Some(*name_span),
            Definition::Struct { name_span, .. } => Some(*name_span),
            Definition::Interface { name_span, .. } => Some(*name_span),
            Definition::Value { name_span, .. } => *name_span,
        }
    }

    pub fn doc(&self) -> Option<&String> {
        match self {
            Definition::TypeAlias { doc, .. }
            | Definition::Enum { doc, .. }
            | Definition::ValueEnum { doc, .. }
            | Definition::Struct { doc, .. }
            | Definition::Interface { doc, .. }
            | Definition::Value { doc, .. } => doc.as_ref(),
        }
    }
}

pub type MethodSignatures = HashMap<EcoString, Type>;

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Public,
    Private,
    Local,
}

impl Visibility {
    pub fn is_public(&self) -> bool {
        matches!(self, Visibility::Public)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub name: EcoString,
    pub generics: Vec<Generic>,
    pub parents: Vec<Type>,
    pub methods: HashMap<EcoString, Type>,
}
