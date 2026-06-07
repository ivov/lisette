use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;

use crate::ast::{
    Annotation, EnumVariant, Generic, Literal, Span, StructFieldDefinition, StructKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TypeAttribute {
    Display,
    ClosedDomain,
    AnonStruct,
}

pub type Attributes = HashMap<TypeAttribute, ()>;

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
        attributes: Attributes,
    },
    Struct {
        generics: Vec<Generic>,
        fields: Vec<StructFieldDefinition>,
        kind: StructKind,
        methods: MethodSignatures,
        constructor: Option<Type>,
        attributes: Attributes,
    },
    Interface {
        definition: Interface,
    },
    Value {
        allowed_lints: Vec<String>,
        go_hints: Vec<String>,
        go_name: Option<String>,
        /// The known literal value when this definition is a case-eligible
        /// constant (usable as a Go `case` and as a const-pattern target).
        /// `None` for variables, functions, and non-literal constants.
        const_value: Option<Literal>,
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

    pub fn is_pointer_backed_newtype<F>(&self, is_alias: F) -> bool
    where
        F: Fn(&str) -> bool,
    {
        self.is_newtype()
            && matches!(
                &self.body,
                DefinitionBody::Struct { fields, .. }
                    if crate::types::peel_alias(&fields[0].ty, is_alias).is_ref()
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

    pub fn const_value(&self) -> Option<&Literal> {
        match &self.body {
            DefinitionBody::Value { const_value, .. } => const_value.as_ref(),
            _ => None,
        }
    }

    pub fn methods_mut(&mut self) -> Option<&mut MethodSignatures> {
        match &mut self.body {
            DefinitionBody::Struct { methods, .. } => Some(methods),
            DefinitionBody::TypeAlias { methods, .. } => Some(methods),
            DefinitionBody::Enum { methods, .. } => Some(methods),
            _ => None,
        }
    }

    pub fn attributes(&self) -> Option<&Attributes> {
        match &self.body {
            DefinitionBody::Struct { attributes, .. } | DefinitionBody::Enum { attributes, .. } => {
                Some(attributes)
            }
            _ => None,
        }
    }

    pub fn is_display(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains_key(&TypeAttribute::Display))
    }

    pub fn is_closed_domain(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains_key(&TypeAttribute::ClosedDomain))
    }

    pub fn is_anon_struct(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains_key(&TypeAttribute::AnonStruct))
    }

    pub fn is_type_definition(&self) -> bool {
        matches!(
            self.body,
            DefinitionBody::Struct { .. }
                | DefinitionBody::Enum { .. }
                | DefinitionBody::TypeAlias { .. }
        )
    }

    pub fn is_type_alias(&self) -> bool {
        matches!(self.body, DefinitionBody::TypeAlias { .. })
    }

    pub fn is_value(&self, qualified_name: &str) -> bool {
        matches!(self.body, DefinitionBody::Value { .. })
            && self.ty.unwrap_forall().get_qualified_id() != Some(qualified_name)
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
