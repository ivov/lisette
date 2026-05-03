use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;

use crate::ast::{
    Annotation, EnumVariant, Generic, Span, StructFieldDefinition, StructKind, ValueEnumVariant,
};
use crate::types::Type;

#[derive(Debug, Clone)]
pub struct Definition {
    pub visibility: Visibility,
    pub ty: Type,
    pub name: Option<EcoString>,
    pub name_span: Option<Span>,
    pub doc: Option<String>,
    pub body: DefinitionBody,
}

#[derive(Debug, Clone)]
pub enum DefinitionBody {
    TypeAlias {
        generics: Vec<Generic>,
        annotation: Annotation,
        methods: MethodSignatures,
    },
    Enum {
        generics: Vec<Generic>,
        variants: Vec<EnumVariant>,
        methods: MethodSignatures,
    },
    ValueEnum {
        variants: Vec<ValueEnumVariant>,
        underlying_ty: Option<Type>,
        methods: MethodSignatures,
    },
    Struct {
        generics: Vec<Generic>,
        fields: Vec<StructFieldDefinition>,
        kind: StructKind,
        methods: MethodSignatures,
        constructor: Option<Type>,
    },
    Interface {
        definition: Interface,
    },
    Value {
        allowed_lints: Vec<String>,
        go_hints: Vec<String>,
        go_name: Option<String>,
    },
}

impl Definition {
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn visibility(&self) -> &Visibility {
        &self.visibility
    }

    pub fn name_span(&self) -> Option<Span> {
        self.name_span
    }

    pub fn doc(&self) -> Option<&String> {
        self.doc.as_ref()
    }

    /// A newtype is a single-field, non-generic tuple struct. Relevant
    /// because Go compiles newtypes to named scalar types, so `.0` is a cast
    /// rather than a field access — it cannot be assigned to, and taking
    /// its address is invalid.
    pub fn is_newtype(&self) -> bool {
        matches!(
            &self.body,
            DefinitionBody::Struct {
                kind: StructKind::Tuple,
                fields,
                generics,
                ..
            } if fields.len() == 1 && generics.is_empty()
        )
    }

    pub fn allowed_lints(&self) -> &[String] {
        match &self.body {
            DefinitionBody::Value { allowed_lints, .. } => allowed_lints,
            _ => &[],
        }
    }

    pub fn go_hints(&self) -> &[String] {
        match &self.body {
            DefinitionBody::Value { go_hints, .. } => go_hints,
            _ => &[],
        }
    }

    pub fn go_name(&self) -> Option<&str> {
        match &self.body {
            DefinitionBody::Value { go_name, .. } => go_name.as_deref(),
            _ => None,
        }
    }

    pub fn methods_mut(&mut self) -> Option<&mut MethodSignatures> {
        match &mut self.body {
            DefinitionBody::Struct { methods, .. } => Some(methods),
            DefinitionBody::TypeAlias { methods, .. } => Some(methods),
            DefinitionBody::Enum { methods, .. } => Some(methods),
            DefinitionBody::ValueEnum { methods, .. } => Some(methods),
            _ => None,
        }
    }

    pub fn is_type_definition(&self) -> bool {
        matches!(
            self.body,
            DefinitionBody::Struct { .. }
                | DefinitionBody::Enum { .. }
                | DefinitionBody::ValueEnum { .. }
                | DefinitionBody::TypeAlias { .. }
        )
    }

    pub fn is_type_alias(&self) -> bool {
        matches!(self.body, DefinitionBody::TypeAlias { .. })
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
