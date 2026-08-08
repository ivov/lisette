use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{Annotation, EnumVariant, Generic, Literal, Span, StructFields};
use crate::types::{
    FunctionParameter, Symbol, Type, build_substitution_map, substitute, type_args_match_params,
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
        methods: Methods,
        attributes: Attributes,
    },
    Enum {
        generics: Vec<Generic>,
        variants: Vec<EnumVariant>,
        methods: Methods,
        attributes: Attributes,
    },
    Struct {
        generics: Vec<Generic>,
        fields: StructFields,
        methods: Methods,
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

    pub fn methods_mut(&mut self) -> Option<&mut Methods> {
        match &mut self.body {
            DefinitionBody::Struct { methods, .. } => Some(methods),
            DefinitionBody::TypeAlias { methods, .. } => Some(methods),
            DefinitionBody::Enum { methods, .. } => Some(methods),
            DefinitionBody::Interface { definition } => Some(&mut definition.methods),
            DefinitionBody::Value { .. } => None,
        }
    }

    pub fn methods(&self) -> Option<&Methods> {
        match &self.body {
            DefinitionBody::Struct { methods, .. }
            | DefinitionBody::TypeAlias { methods, .. }
            | DefinitionBody::Enum { methods, .. } => Some(methods),
            DefinitionBody::Interface { definition } => Some(&definition.methods),
            DefinitionBody::Value { .. } => None,
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
            .is_some_and(|method| is_ufcs_method_type(&method.ty, base_generics_count))
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

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Method {
    pub source_name: EcoString,
    pub ty: Type,
    pub visibility: Visibility,
    pub name_span: Option<Span>,
    pub doc: Option<String>,
    pub allowed_lints: Vec<String>,
    pub go_hints: Vec<String>,
}

impl Method {
    pub fn with_type(&self, ty: Type) -> Self {
        Self { ty, ..self.clone() }
    }

    fn with_receiver_placeholder(self) -> Self {
        let Self {
            source_name,
            ty,
            visibility,
            name_span,
            doc,
            allowed_lints,
            go_hints,
        } = self;
        Self {
            source_name,
            ty: ty.with_receiver_placeholder(),
            visibility,
            name_span,
            doc,
            allowed_lints,
            go_hints,
        }
    }
}

pub type Methods = HashMap<EcoString, Method>;

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceInstance {
    pub ty: Type,
    pub parent_of: Option<Symbol>,
}

/// Instantiate an interface and its complete parent hierarchy exactly once.
/// Package-qualified structural identity prevents same-named interfaces from
/// collapsing, while the active path guard terminates malformed cycles.
pub fn interface_instances<'d, F>(interface_ty: &Type, lookup: F) -> Vec<InterfaceInstance>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    fn collect<'d, F>(
        interface_ty: &Type,
        parent_of: Option<&Symbol>,
        lookup: F,
        visited: &mut HashSet<String>,
        visiting: &mut HashSet<Symbol>,
        instances: &mut Vec<InterfaceInstance>,
    ) where
        F: Copy + Fn(&str) -> Option<&'d Definition>,
    {
        let resolved = crate::types::peel_alias(interface_ty, lookup);
        let Type::Nominal { id, params } = &resolved else {
            return;
        };
        // `Display` intentionally omits package qualification, so it cannot
        // distinguish same-named interfaces from different packages.
        if !visited.insert(format!("{resolved:?}")) || !visiting.insert(id.clone()) {
            return;
        }
        let Some(Definition {
            body: DefinitionBody::Interface { definition },
            ..
        }) = lookup(id)
        else {
            visiting.remove(id);
            return;
        };
        let map = build_substitution_map(&definition.generics, params);
        instances.push(InterfaceInstance {
            ty: resolved.clone(),
            parent_of: parent_of.cloned(),
        });
        for parent in &definition.parents {
            collect(
                &substitute(parent, &map),
                Some(id),
                lookup,
                visited,
                visiting,
                instances,
            );
        }
        visiting.remove(id);
    }

    let mut instances = Vec::new();
    collect(
        interface_ty,
        None,
        lookup,
        &mut HashSet::default(),
        &mut HashSet::default(),
        &mut instances,
    );
    instances
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceRequirement {
    pub declaring_interface: Symbol,
    pub parent_of: Option<Symbol>,
    pub name: EcoString,
    /// The method as declared. Its type determines the generic declaration's
    /// physical ABI even when substitution reveals a special logical type.
    pub method: Method,
    /// The logical signature after applying all interface type arguments.
    pub ty: Type,
}

/// Flatten an interface and its instantiated parents into declaration-tagged
/// method requirements. Cycles and repeated generic instantiations are handled
/// here so registration, inference, and emission cannot disagree about the
/// inherited signatures.
pub fn interface_requirements<'d, F>(interface_ty: &Type, lookup: F) -> Vec<InterfaceRequirement>
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    let mut requirements = Vec::new();
    for instance in interface_instances(interface_ty, lookup) {
        let Type::Nominal { id, params } = instance.ty else {
            continue;
        };
        let Some(Definition {
            body: DefinitionBody::Interface { definition },
            ..
        }) = lookup(&id)
        else {
            continue;
        };
        let map = build_substitution_map(&definition.generics, &params);
        requirements.extend(
            definition
                .methods
                .iter()
                .map(|(name, method)| InterfaceRequirement {
                    declaring_interface: id.clone(),
                    parent_of: instance.parent_of.clone(),
                    name: name.clone(),
                    method: method.clone(),
                    ty: substitute(&method.ty, &map),
                }),
        );
    }
    requirements
}

/// Resolve the complete method set for a type, including inherited and alias methods.
pub fn methods_for_type<'d, F>(
    ty: &Type,
    trait_bounds: &HashMap<crate::types::Symbol, Vec<Type>>,
    lookup: F,
) -> Methods
where
    F: Copy + Fn(&str) -> Option<&'d Definition>,
{
    fn collect<'d, F>(
        ty: &Type,
        trait_bounds: &HashMap<crate::types::Symbol, Vec<Type>>,
        lookup: F,
        visited: &mut HashSet<String>,
    ) -> Methods
    where
        F: Copy + Fn(&str) -> Option<&'d Definition>,
    {
        let stripped = ty.strip_refs();
        let Some(qualified_name) = method_lookup_key(&stripped) else {
            return Methods::default();
        };

        if !visited.insert(qualified_name.as_str().to_string()) {
            return Methods::default();
        }

        if lookup(&qualified_name)
            .is_some_and(|definition| matches!(definition.body, DefinitionBody::Interface { .. }))
        {
            return interface_requirements(&stripped, lookup)
                .into_iter()
                .map(|requirement| {
                    (
                        requirement.name,
                        requirement
                            .method
                            .with_type(requirement.ty)
                            .with_receiver_placeholder(),
                    )
                })
                .collect();
        }

        if let Some(bounds) = trait_bounds.get(&qualified_name) {
            return bounds
                .iter()
                .flat_map(|bound| collect(bound, trait_bounds, lookup, visited))
                .collect();
        }

        let mut methods = lookup(&qualified_name)
            .and_then(Definition::methods)
            .cloned()
            .unwrap_or_default();

        if lookup(&qualified_name).is_some_and(Definition::is_transparent_type_alias) {
            let underlying = crate::types::peel_alias(&stripped, lookup);
            if underlying != stripped {
                for (name, method) in collect(&underlying, trait_bounds, lookup, visited) {
                    methods.entry(name).or_insert(method);
                }
            }
        }

        methods
    }

    collect(ty, trait_bounds, lookup, &mut HashSet::default())
}

fn method_lookup_key(ty: &Type) -> Option<crate::types::Symbol> {
    match ty {
        Type::Nominal { id, .. } => Some(id.clone()),
        Type::Compound { kind, .. } => Some(Symbol::from_parts("prelude", kind.leaf_name())),
        Type::Simple(kind) => Some(Symbol::from_parts("prelude", kind.leaf_name())),
        Type::Array { .. } => Some(Symbol::from_parts("prelude", "Array")),
        _ => None,
    }
}

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
    pub methods: Methods,
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
                methods: HashMap::from_iter([(
                    "map".into(),
                    Method {
                        source_name: "map".into(),
                        ty: method_ty,
                        visibility: Visibility::Public,
                        name_span: None,
                        doc: None,
                        allowed_lints: vec![],
                        go_hints: vec![],
                    },
                )]),
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
