use crate::target::names::go_name;
use crate::target::names::packages::{PackageRequirements, PackageUse};
use crate::target::names::receiver::ReceiverGenerics;
use std::iter::once;
use std::{fmt::Display, slice};
use syntax::types::Type;

pub(crate) fn is_prelude_type(name: &str) -> bool {
    matches!(
        name,
        "Option"
            | "Result"
            | "Partial"
            | "Range"
            | "RangeInclusive"
            | "RangeFrom"
            | "RangeTo"
            | "RangeToInclusive"
            | "PanicValue"
    )
}

pub(crate) fn returns_go_void(return_ty: &Type) -> bool {
    return_ty.is_unit() || return_ty.is_never()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelDirection {
    Bidirectional,
    Send,
    Receive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GoTypeShape {
    Empty,
    Name(String),
    Qualified {
        package: String,
        member: String,
    },
    Instantiated {
        base: Box<GoType>,
        arguments: Vec<GoType>,
    },
    Arguments(Vec<GoType>),
    Results(Vec<GoType>),
    Slice(Box<GoType>),
    Map(Box<GoType>, Box<GoType>),
    Pointer(Box<GoType>),
    Array(String, Box<GoType>),
    Channel(ChannelDirection, Box<GoType>),
    Variadic(Box<GoType>),
    Function(Vec<GoType>, Option<Box<GoType>>),
    Struct(Vec<(String, GoType)>),
    Interface(Vec<(String, GoType)>),
    EmptyInterface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoTypeKind {
    Value,
    Slice,
    Map,
    Array,
    PointerToArray,
    Channel,
}

#[derive(Debug, Clone)]
pub(crate) struct GoType {
    pub(crate) shape: Box<GoTypeShape>,
    pub(crate) alias: Option<Box<GoType>>,
    requirements: PackageRequirements,
    physical_kind: Option<GoTypeKind>,
}

impl PartialEq for GoType {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape
    }
}

impl Eq for GoType {}

impl Default for GoType {
    fn default() -> Self {
        Self::of_shape(GoTypeShape::Empty)
    }
}

impl GoType {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self::of_shape(match name.rsplit_once('.') {
            Some((package, member)) => GoTypeShape::Qualified {
                package: package.to_string(),
                member: member.to_string(),
            },
            None => GoTypeShape::Name(name),
        })
    }

    pub(crate) fn any() -> Self {
        Self::of_shape(GoTypeShape::EmptyInterface)
    }

    fn of_shape(shape: GoTypeShape) -> Self {
        Self::structured(shape, [])
    }

    fn structured<'argument>(
        shape: GoTypeShape,
        dependencies: impl IntoIterator<Item = &'argument GoType>,
    ) -> Self {
        let mut value = Self {
            shape: Box::new(shape),
            alias: None,
            requirements: PackageRequirements::default(),
            physical_kind: None,
        };

        value.merge_all(dependencies);
        value
    }

    pub(crate) fn named(member: &str, arguments: &[GoType], package: Option<PackageUse>) -> Self {
        let mut base = if let Some(package) = package {
            let mut base = Self::structured(
                GoTypeShape::Qualified {
                    package: package.qualifier().to_string(),
                    member: member.to_string(),
                },
                [],
            );

            base.requirements.require(package);
            base
        } else {
            Self::new(member)
        };

        if !arguments.is_empty() {
            let shape = GoTypeShape::Instantiated {
                base: Box::new(base.clone()),
                arguments: arguments.to_vec(),
            };
            base = Self::structured(shape, once(&base).chain(arguments));
        }
        base
    }

    fn merge(&mut self, other: &GoType) {
        self.requirements.extend(&other.requirements);
    }

    fn merge_all<'argument>(&mut self, others: impl IntoIterator<Item = &'argument GoType>) {
        for other in others {
            self.merge(other);
        }
    }

    pub(crate) fn requirements(&self) -> &PackageRequirements {
        &self.requirements
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self.shape.as_ref(), GoTypeShape::Empty)
    }

    pub(crate) fn compared_as(self, target: GoType) -> Self {
        let mut compared = self.clone();
        compared.shape = target.shape;
        compared.alias = Some(Box::new(self));
        compared
    }

    pub(crate) fn leading_name(&self) -> &str {
        if let Some(alias) = &self.alias {
            return alias.leading_name();
        }
        match self.shape.as_ref() {
            GoTypeShape::Name(name) => name,
            GoTypeShape::Qualified { package, .. } => package,
            GoTypeShape::Instantiated { base, .. } => base.leading_name(),
            GoTypeShape::Pointer(inner) | GoTypeShape::Variadic(inner) => inner.leading_name(),
            GoTypeShape::Results(results) => results.first().map_or("", GoType::leading_name),
            GoTypeShape::Map(..) => "map",
            GoTypeShape::Function(..) => "func",
            GoTypeShape::Channel(..) => "chan",
            GoTypeShape::Struct(_) => "struct",
            GoTypeShape::Interface(_) => "interface",
            GoTypeShape::EmptyInterface => "any",
            GoTypeShape::Empty
            | GoTypeShape::Arguments(_)
            | GoTypeShape::Slice(_)
            | GoTypeShape::Array(..) => "",
        }
    }

    pub(crate) fn is_named(&self, name: &str) -> bool {
        matches!(self.shape.as_ref(), GoTypeShape::Name(actual) if actual == name)
    }

    pub(crate) fn is_channel(&self) -> bool {
        self.physical_kind() == GoTypeKind::Channel
    }

    pub(crate) fn physical_kind(&self) -> GoTypeKind {
        self.physical_kind
            .unwrap_or_else(|| match self.shape.as_ref() {
                GoTypeShape::Slice(_) => GoTypeKind::Slice,
                GoTypeShape::Map(..) => GoTypeKind::Map,
                GoTypeShape::Array(..) => GoTypeKind::Array,
                GoTypeShape::Pointer(inner) if inner.physical_kind() == GoTypeKind::Array => {
                    GoTypeKind::PointerToArray
                }
                GoTypeShape::Channel(..) => GoTypeKind::Channel,
                _ => GoTypeKind::Value,
            })
    }

    pub(crate) fn with_physical_kind(mut self, kind: GoTypeKind) -> Self {
        self.physical_kind = Some(kind);
        self
    }

    pub(crate) fn arguments(&self) -> &[GoType] {
        match self.shape.as_ref() {
            GoTypeShape::Arguments(arguments) => arguments,
            _ => &[],
        }
    }

    pub(crate) fn type_arguments(arguments: &[GoType]) -> Self {
        if arguments.is_empty() {
            return Self::default();
        }
        Self::structured(GoTypeShape::Arguments(arguments.to_vec()), arguments)
    }

    pub(crate) fn type_argument(argument: &GoType) -> Self {
        Self::type_arguments(slice::from_ref(argument))
    }

    pub(crate) fn result_list(results: &[GoType]) -> Self {
        Self::structured(GoTypeShape::Results(results.to_vec()), results)
    }

    pub(crate) fn function(parameters: &[GoType], result: Option<&GoType>) -> Self {
        Self::structured(
            GoTypeShape::Function(parameters.to_vec(), result.cloned().map(Box::new)),
            parameters.iter().chain(result),
        )
    }

    pub(crate) fn slice_of(element: &GoType) -> Self {
        Self::structured(GoTypeShape::Slice(Box::new(element.clone())), [element])
    }

    pub(crate) fn map_of(key: &GoType, value: &GoType) -> Self {
        Self::structured(
            GoTypeShape::Map(Box::new(key.clone()), Box::new(value.clone())),
            [key, value],
        )
    }

    pub(crate) fn pointer_to(pointee: &GoType) -> Self {
        Self::structured(GoTypeShape::Pointer(Box::new(pointee.clone())), [pointee])
    }

    pub(crate) fn array_of(length: impl Display, element: &GoType) -> Self {
        Self::structured(
            GoTypeShape::Array(length.to_string(), Box::new(element.clone())),
            [element],
        )
    }

    pub(crate) fn channel(direction: ChannelDirection, element: &GoType) -> Self {
        Self::structured(
            GoTypeShape::Channel(direction, Box::new(element.clone())),
            [element],
        )
    }

    pub(crate) fn variadic(element: &GoType) -> Self {
        Self::structured(GoTypeShape::Variadic(Box::new(element.clone())), [element])
    }

    pub(crate) fn structure(fields: Vec<(String, GoType)>) -> Self {
        Self::structured(
            GoTypeShape::Struct(fields.clone()),
            fields.iter().map(|(_, ty)| ty),
        )
    }

    pub(crate) fn method_interface(name: &str, parameters: &[GoType], result: &GoType) -> Self {
        let signature = Self::function(parameters, (!result.is_empty()).then_some(result));
        Self::structured(
            GoTypeShape::Interface(vec![(name.to_string(), signature.clone())]),
            [&signature],
        )
    }

    pub(crate) fn prelude(name: &str, arguments: &[GoType]) -> Self {
        Self::named(
            name,
            arguments,
            Some(PackageUse::generated(go_name::GeneratedPackage::Prelude)),
        )
    }
}

pub(crate) fn receiver_type(generics: &ReceiverGenerics, type_name: &str) -> GoType {
    GoType::named(
        &go_name::escape_type_name(type_name),
        &generics.0.iter().map(GoType::new).collect::<Vec<_>>(),
        None,
    )
}
