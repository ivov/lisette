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

fn is_plain_type_name(code: &str) -> bool {
    code.chars()
        .all(|character| character.is_alphanumeric() || character == '_' || character == '.')
}

pub(crate) fn conversion_prefix(go_type: &str) -> String {
    if is_plain_type_name(go_type) {
        format!("{}(", go_type)
    } else {
        format!("({})(", go_type)
    }
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
    Raw(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoTypeKind {
    Value,
    Slice,
    Map,
    Array,
    PointerToArray,
    Channel,
    Unclassified,
}

#[derive(Debug, Clone)]
pub(crate) struct GoType {
    pub(crate) code: String,
    pub(crate) shape: Box<GoTypeShape>,
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
        Self::new("")
    }
}

impl GoType {
    pub(crate) fn new(code: impl Into<String>) -> Self {
        let code = code.into();
        let shape = match code.as_str() {
            "" => GoTypeShape::Empty,
            "struct{}" => GoTypeShape::Struct(Vec::new()),
            "any" | "interface{}" => GoTypeShape::EmptyInterface,
            name if is_plain_type_name(name) => match name.rsplit_once('.') {
                Some((package, member)) => GoTypeShape::Qualified {
                    package: package.to_string(),
                    member: member.to_string(),
                },
                None => GoTypeShape::Name(name.to_string()),
            },
            _ => GoTypeShape::Raw(code.clone()),
        };
        Self {
            code,
            shape: Box::new(shape),
            requirements: PackageRequirements::default(),
            physical_kind: None,
        }
    }

    fn structured<'argument>(
        code: String,
        shape: GoTypeShape,
        dependencies: impl IntoIterator<Item = &'argument GoType>,
    ) -> Self {
        let mut value = Self {
            code,
            shape: Box::new(shape),
            requirements: PackageRequirements::default(),
            physical_kind: None,
        };
        value.merge_all(dependencies);
        value
    }

    pub(crate) fn named(member: &str, arguments: &[GoType], package: Option<PackageUse>) -> Self {
        let mut base = if let Some(package) = package {
            let code = format!("{}.{member}", package.qualifier());
            let mut base = Self::structured(
                code,
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
            let code = format!("{}{}", base.code, Self::type_arguments(arguments).code);
            let shape = GoTypeShape::Instantiated {
                base: Box::new(base.clone()),
                arguments: arguments.to_vec(),
            };
            base = Self::structured(code, shape, once(&base).chain(arguments));
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
    pub(crate) fn compared_as(mut self, target: GoType) -> Self {
        self.shape = target.shape;
        self
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
                GoTypeShape::Raw(_) => GoTypeKind::Unclassified,
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
        let code = format!(
            "[{}]",
            arguments
                .iter()
                .map(|argument| argument.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        Self::structured(code, GoTypeShape::Arguments(arguments.to_vec()), arguments)
    }
    pub(crate) fn type_argument(argument: &GoType) -> Self {
        Self::type_arguments(slice::from_ref(argument))
    }
    pub(crate) fn result_list(results: &[GoType]) -> Self {
        let code = format!(
            "({})",
            results
                .iter()
                .map(|result| result.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        Self::structured(code, GoTypeShape::Results(results.to_vec()), results)
    }
    pub(crate) fn function(parameters: &[GoType], result: Option<&GoType>) -> Self {
        let parameter_text = parameters
            .iter()
            .map(|parameter| parameter.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let code = match result {
            Some(result) => format!("func({parameter_text}) {}", result.code),
            None => format!("func({parameter_text})"),
        };
        Self::structured(
            code,
            GoTypeShape::Function(parameters.to_vec(), result.cloned().map(Box::new)),
            parameters.iter().chain(result),
        )
    }
    pub(crate) fn slice_of(element: &GoType) -> Self {
        Self::structured(
            format!("[]{}", element.code),
            GoTypeShape::Slice(Box::new(element.clone())),
            [element],
        )
    }
    pub(crate) fn map_of(key: &GoType, value: &GoType) -> Self {
        Self::structured(
            format!("map[{}]{}", key.code, value.code),
            GoTypeShape::Map(Box::new(key.clone()), Box::new(value.clone())),
            [key, value],
        )
    }
    pub(crate) fn parenthesized(&self) -> Self {
        let mut result = self.clone();
        result.code = format!("({})", self.code);
        result
    }
    pub(crate) fn pointer_to(pointee: &GoType) -> Self {
        Self::structured(
            format!("*{}", pointee.code),
            GoTypeShape::Pointer(Box::new(pointee.clone())),
            [pointee],
        )
    }
    pub(crate) fn array_of(length: impl Display, element: &GoType) -> Self {
        Self::structured(
            format!("[{length}]{}", element.code),
            GoTypeShape::Array(length.to_string(), Box::new(element.clone())),
            [element],
        )
    }
    pub(crate) fn channel(direction: ChannelDirection, element: &GoType) -> Self {
        let prefix = match direction {
            ChannelDirection::Send => "chan<-",
            ChannelDirection::Receive => "<-chan",
            ChannelDirection::Bidirectional => "chan",
        };
        Self::structured(
            format!("{prefix} {}", element.code),
            GoTypeShape::Channel(direction, Box::new(element.clone())),
            [element],
        )
    }
    pub(crate) fn variadic(element: &GoType) -> Self {
        Self::structured(
            format!("...{}", element.code),
            GoTypeShape::Variadic(Box::new(element.clone())),
            [element],
        )
    }
    pub(crate) fn structure(fields: Vec<(String, GoType)>) -> Self {
        let code = if fields.is_empty() {
            "struct{}".to_string()
        } else {
            format!(
                "struct {{ {} }}",
                fields
                    .iter()
                    .map(|(name, ty)| format!("{name} {}", ty.code))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        };
        Self::structured(
            code,
            GoTypeShape::Struct(fields.clone()),
            fields.iter().map(|(_, ty)| ty),
        )
    }
    pub(crate) fn method_interface(name: &str, parameters: &[GoType], result: &GoType) -> Self {
        let signature = Self::function(parameters, (!result.is_empty()).then_some(result));
        let code = format!(
            "interface {{ {name}({}) {} }}",
            parameters
                .iter()
                .map(|parameter| parameter.code.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            result.code
        );
        Self::structured(
            code,
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
