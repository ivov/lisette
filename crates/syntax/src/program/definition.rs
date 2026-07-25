use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{Annotation, EnumVariant, Generic, Literal, Span, StructFields};
use crate::types::{FunctionParameter, Type, build_substitution_map, substitute};

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
    /// Go struct that looks flat but hides an embed bindgen could not emit, so it
    /// cannot be soundly embedded.
    HiddenEmbed,
    Serialized,
}

pub type Attributes = HashSet<TypeAttribute>;

#[derive(Debug, Clone)]
pub enum ValueKind {
    Runtime,
    ConstantDeclaration,
    Constant(Literal),
}

#[derive(Debug, Clone)]
pub enum DefinitionBody {
    TypeAlias {
        generics: Vec<Generic>,
        alias: AliasKind,
        methods: MethodSignatures,
        attributes: Attributes,
    },
    Enum {
        generics: Vec<Generic>,
        variants: Vec<EnumVariant>,
        methods: MethodSignatures,
        attributes: Attributes,
    },
    Struct {
        generics: Vec<Generic>,
        fields: StructFields,
        methods: MethodSignatures,
        attributes: Attributes,
    },
    Interface {
        definition: Interface,
    },
    Value {
        kind: ValueKind,
        allowed_lints: Vec<String>,
        go_hints: Vec<String>,
        go_name: Option<String>,
        /// Go's full type-parameter list for a `#[go(collapsed_type_params)]`
        /// function, in declaration order, each entry as a Lisette type (e.g.
        /// `"Slice<E>, E"`). Lets emit rebuild Go's type arguments when the
        /// collapsed Lisette list cannot be projected onto Go's positionally.
        go_type_param_recipe: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum AliasKind {
    Opaque(Annotation),
    Transparent {
        annotation: Annotation,
        target: Type,
    },
}

impl AliasKind {
    pub fn annotation(&self) -> &Annotation {
        match self {
            Self::Opaque(annotation) | Self::Transparent { annotation, .. } => annotation,
        }
    }
}

impl DefinitionBody {
    pub fn generics(&self) -> Option<&[Generic]> {
        match self {
            Self::TypeAlias { generics, .. }
            | Self::Enum { generics, .. }
            | Self::Struct { generics, .. } => Some(generics),
            Self::Interface { definition } => Some(&definition.generics),
            Self::Value { .. } => None,
        }
    }

    pub fn generics_mut(&mut self) -> Option<&mut [Generic]> {
        match self {
            Self::TypeAlias { generics, .. }
            | Self::Enum { generics, .. }
            | Self::Struct { generics, .. } => Some(generics),
            Self::Interface { definition } => Some(&mut definition.generics),
            Self::Value { .. } => None,
        }
    }
}

impl Definition {
    /// A newtype is a single-field, non-generic tuple struct. Relevant
    /// because Go compiles newtypes to named scalar types, so `.0` is a cast
    /// rather than a field access — it cannot be assigned to, and taking
    /// its address is invalid.
    pub fn is_newtype(&self) -> bool {
        matches!(
            &self.body,
            DefinitionBody::Struct {
                fields: StructFields::Tuple(fields),
                generics,
                ..
            } if fields.len() == 1 && generics.is_empty()
        )
    }

    pub fn is_pointer_backed_newtype<'d, F>(&self, lookup: F) -> bool
    where
        F: Fn(&str) -> Option<&'d Definition>,
    {
        self.is_newtype()
            && matches!(
                &self.body,
                DefinitionBody::Struct {
                    fields: StructFields::Tuple(fields),
                    ..
                } if crate::types::peel_alias(&fields[0].ty, lookup).is_ref()
            )
    }

    pub fn instantiate_alias_target(&self, params: &[Type]) -> Option<Type> {
        let DefinitionBody::TypeAlias {
            generics,
            alias: AliasKind::Transparent { target, .. },
            ..
        } = &self.body
        else {
            return None;
        };
        Some(substitute(
            target,
            &build_substitution_map(generics, params),
        ))
    }

    pub fn instantiate_underlying(&self, params: &[Type]) -> Option<Type> {
        if let Some(target) = self.instantiate_alias_target(params) {
            return Some(target);
        }
        match &self.body {
            DefinitionBody::Struct {
                fields: StructFields::Tuple(fields),
                ..
            } if self.is_newtype() => Some(fields[0].ty.clone()),
            _ => None,
        }
    }

    /// Returns the callable type of a tuple struct constructor.
    ///
    /// The constructor is derived from the struct's resolved fields and type
    /// rather than stored separately, so it cannot become stale when either
    /// source changes.
    pub fn constructor_type(&self) -> Option<Type> {
        let DefinitionBody::Struct {
            fields: StructFields::Tuple(fields),
            generics,
            ..
        } = &self.body
        else {
            return None;
        };

        let return_type = self.ty.unwrap_forall().clone();
        let function = Type::function(
            fields
                .iter()
                .map(|field| FunctionParameter::new(field.ty.clone(), false))
                .collect(),
            Default::default(),
            return_type.into(),
        );

        Some(if generics.is_empty() {
            function
        } else {
            Type::Forall {
                vars: generics
                    .iter()
                    .map(|generic| generic.name.clone())
                    .collect(),
                body: Box::new(function),
            }
        })
    }

    pub fn is_transparent_type_alias(&self) -> bool {
        matches!(
            self.body,
            DefinitionBody::TypeAlias {
                alias: AliasKind::Transparent { .. },
                ..
            }
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

    pub fn go_type_param_recipe(&self) -> Option<&str> {
        match &self.body {
            DefinitionBody::Value {
                go_type_param_recipe,
                ..
            } => go_type_param_recipe.as_deref(),
            _ => None,
        }
    }

    pub fn const_value(&self) -> Option<&Literal> {
        match &self.body {
            DefinitionBody::Value {
                kind: ValueKind::Constant(value),
                ..
            } => Some(value),
            _ => None,
        }
    }

    pub fn is_const(&self) -> bool {
        matches!(
            self.body,
            DefinitionBody::Value {
                kind: ValueKind::ConstantDeclaration | ValueKind::Constant(_),
                ..
            }
        )
    }

    pub fn methods_mut(&mut self) -> Option<&mut MethodSignatures> {
        match &mut self.body {
            DefinitionBody::Struct { methods, .. } => Some(methods),
            DefinitionBody::TypeAlias { methods, .. } => Some(methods),
            DefinitionBody::Enum { methods, .. } => Some(methods),
            _ => None,
        }
    }

    fn attributes(&self) -> Option<&Attributes> {
        match &self.body {
            DefinitionBody::Struct { attributes, .. }
            | DefinitionBody::Enum { attributes, .. }
            | DefinitionBody::TypeAlias { attributes, .. } => Some(attributes),
            _ => None,
        }
    }

    pub fn is_display(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::Display))
    }

    pub fn is_closed_domain(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::ClosedDomain))
    }

    pub fn is_serialized(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::Serialized))
    }

    pub fn is_anon_struct(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::AnonStruct))
    }

    pub fn has_hidden_embed(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::HiddenEmbed))
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
