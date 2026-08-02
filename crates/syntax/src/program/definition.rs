use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{Annotation, EnumVariant, Generic, Literal, Span, StructFields};
use crate::types::{
    FunctionParameter, Type, build_substitution_map, substitute, type_args_match_params,
};

#[derive(Debug, Clone)]
pub struct Definition {
    pub visibility: Visibility,
    pub ty: Type,
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
    ZeroSafe,
    /// Go type whose zero value is documented broken, so it must come from
    /// its constructor.
    ZeroUnsafe,
    HiddenFields,
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
    /// rather than a field access, it cannot be assigned to, and taking
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

    pub fn methods(&self) -> Option<&MethodSignatures> {
        match &self.body {
            DefinitionBody::Struct { methods, .. }
            | DefinitionBody::TypeAlias { methods, .. }
            | DefinitionBody::Enum { methods, .. } => Some(methods),
            DefinitionBody::Interface { .. } | DefinitionBody::Value { .. } => None,
        }
    }

    pub fn is_ufcs_method(&self, method: &str) -> bool {
        let (methods, base_generics_count) = match &self.body {
            DefinitionBody::Struct {
                methods, generics, ..
            }
            | DefinitionBody::Enum {
                methods, generics, ..
            }
            | DefinitionBody::TypeAlias {
                methods, generics, ..
            } => (methods, generics.len()),
            DefinitionBody::Interface { .. } | DefinitionBody::Value { .. } => return false,
        };
        methods
            .get(method)
            .is_some_and(|method_ty| is_ufcs_method_type(method_ty, base_generics_count))
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

    pub fn is_zero_safe(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::ZeroSafe))
    }

    pub fn is_zero_unsafe(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::ZeroUnsafe))
    }

    pub fn has_hidden_fields(&self) -> bool {
        self.attributes()
            .is_some_and(|a| a.contains(&TypeAttribute::HiddenFields))
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

fn is_ufcs_method_type(method_ty: &Type, base_generics_count: usize) -> bool {
    let Type::Forall { vars, body } = method_ty else {
        return base_generics_count > 0;
    };

    if vars.len() > base_generics_count {
        return true;
    }

    if let Type::Function(function) = body.as_ref()
        && let Some(receiver) = function.params.first()
        && let Type::Nominal {
            params: receiver_params,
            ..
        } = receiver.ty.strip_refs()
        && !type_args_match_params(&receiver_params, vars.iter())
    {
        return true;
    }

    false
}

pub type MethodSignatures = HashMap<EcoString, Type>;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    pub generics: Vec<Generic>,
    pub parents: Vec<Type>,
    pub methods: HashMap<EcoString, Type>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Symbol;

    fn generic(name: &str) -> Generic {
        Generic::new(name, vec![], Span::dummy())
    }

    fn receiver(params: Vec<Type>) -> Type {
        Type::Nominal {
            id: Symbol::from_raw("m.Box"),
            params,
        }
    }

    fn method(vars: &[&str], receiver: Type) -> Type {
        let function = Type::function(
            vec![FunctionParameter::new(receiver, false)],
            Default::default(),
            Box::new(Type::unit()),
        );
        Type::Forall {
            vars: vars.iter().map(|name| EcoString::from(*name)).collect(),
            body: Box::new(function),
        }
    }

    fn definition(method_ty: Type) -> Definition {
        Definition {
            visibility: Visibility::Public,
            ty: receiver(vec![Type::Parameter("T".into())]),
            name_span: None,
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![generic("T")],
                fields: StructFields::Record(vec![]),
                methods: HashMap::from_iter([("map".into(), method_ty)]),
                attributes: Attributes::default(),
            },
        }
    }

    #[test]
    fn full_generic_receiver_is_a_selector_method() {
        let definition = definition(method(&["T"], receiver(vec![Type::Parameter("T".into())])));

        assert!(!definition.is_ufcs_method("map"));
    }

    #[test]
    fn extra_method_generic_requires_ufcs() {
        let definition = definition(method(
            &["T", "U"],
            receiver(vec![Type::Parameter("T".into())]),
        ));

        assert!(definition.is_ufcs_method("map"));
    }

    #[test]
    fn specialized_receiver_requires_ufcs() {
        let definition = definition(method(&["T"], receiver(vec![Type::int()])));

        assert!(definition.is_ufcs_method("map"));
    }
}
