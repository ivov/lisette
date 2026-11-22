use crate::EmitFacts;
use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::layout::{FunctionLayout, ValueLayout};
use crate::ir::values::{ConstantKind, GoExpression};
use crate::names::go_name;
use crate::names::packages::{PackageRequirements, PackageUse};
use std::borrow::Cow;
use std::iter::once;
use std::{fmt::Display, slice};
use syntax::program::GoTypeRecipe;
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{
    CompoundKind, SimpleKind, SubstitutionMap, Type, peel_underlying, unqualified_name,
};

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
    fn with_physical_kind(mut self, kind: GoTypeKind) -> Self {
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

fn physical_type_kind<'argument>(
    ty: &Type,
    lookup: impl Fn(&str) -> Option<&'argument Definition>,
) -> GoTypeKind {
    match peel_underlying(ty, &lookup) {
        Type::Compound {
            kind: CompoundKind::Slice,
            ..
        } => GoTypeKind::Slice,
        Type::Compound {
            kind: CompoundKind::Map,
            ..
        } => GoTypeKind::Map,
        Type::Compound {
            kind: CompoundKind::Channel | CompoundKind::Sender | CompoundKind::Receiver,
            ..
        } => GoTypeKind::Channel,
        Type::Compound {
            kind: CompoundKind::Ref,
            args: arguments,
            ..
        } if arguments
            .first()
            .is_some_and(|inner| matches!(peel_underlying(inner, &lookup), Type::Array { .. })) =>
        {
            GoTypeKind::PointerToArray
        }
        Type::Array { .. } => GoTypeKind::Array,
        _ => GoTypeKind::Value,
    }
}

impl Planner<'_> {
    pub(crate) fn plain_literal_go_type(&self, ty: &Type) -> Option<GoType> {
        let peeled = self.facts.peel_alias(ty);
        match &peeled {
            Type::Simple(_) => {}
            Type::Nominal { params, .. } if params.is_empty() => {}
            _ => return None,
        }
        Some(self.go_type(&peeled))
    }

    pub(crate) fn target_go_type(&self, ty: &Type) -> GoType {
        self.go_type(&self.facts.peel_alias(ty))
    }

    pub(crate) fn go_type(&self, ty: &Type) -> GoType {
        self.value_layout(ty, SlotOrigin::Lisette).go_type(self)
    }

    fn leaf_go_type(&self, ty: &Type) -> GoType {
        match ty {
            Type::Nominal {
                id,
                params: parameters,
                ..
            } => self.constructor_go_type(id, parameters, ty),
            Type::Simple(kind) => {
                if matches!(kind, SimpleKind::Unit) {
                    GoType::new("struct{}")
                } else {
                    GoType::new(kind.leaf_name().to_string())
                }
            }
            Type::Compound {
                kind,
                args: arguments,
                ..
            } => self.compound_go_type(*kind, arguments, ty),
            Type::Var { .. } | Type::Uninferred | Type::Ignored => GoType::new("any"),
            Type::Forall { .. } => GoType::new("any"),
            Type::Parameter(name) => GoType::new(self.generic_go_name(name).into_owned()),
            Type::Never => GoType::new("struct{}"),
            Type::Error => self.reject_untyped_position("an error type"),
            Type::ImportNamespace(_) => self.reject_untyped_position("a package name"),
            Type::ReceiverPlaceholder => GoType::new("any"),
            Type::Function(_) | Type::Tuple(_) | Type::Array { .. } => {
                unreachable!("the value layout renders a structural type")
            }
        }
    }

    fn compound_go_type(&self, kind: CompoundKind, arguments: &[Type], ty: &Type) -> GoType {
        match kind {
            CompoundKind::Ref => unreachable!("the value layout renders a reference"),
            CompoundKind::VarArgs => {
                let element =
                    self.go_type(arguments.first().expect("varargs have an element type"));
                GoType::variadic(&element)
            }
            _ => {
                let native = NativeTypeKind::from_type(ty)
                    .expect("every other compound type is a native Go type");
                self.native_go_type(native, ty)
            }
        }
    }

    pub(crate) fn format_type_args(&self, parameters: &[Type]) -> GoType {
        self.format_type_arg_iter(parameters)
    }

    pub(crate) fn format_type_arg_iter<'t>(
        &self,
        parameters: impl IntoIterator<Item = &'t Type>,
    ) -> GoType {
        let arguments: Vec<GoType> = parameters
            .into_iter()
            .map(|param| self.go_type(param))
            .collect();
        GoType::type_arguments(&arguments)
    }

    pub(crate) fn reconstruct_collapsed_type_args(
        &self,
        recipe: &[GoTypeRecipe],
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let parts = recipe
            .iter()
            .map(|entry| self.render_recipe_entry(entry, mapping))
            .collect::<Option<Vec<_>>>()?;
        (!parts.is_empty()).then(|| GoType::type_arguments(&parts))
    }

    fn render_recipe_entry(
        &self,
        entry: &GoTypeRecipe,
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        Some(match entry {
            GoTypeRecipe::Parameter(name) => self.go_type(mapping.get(name)?),
            GoTypeRecipe::Slice(element) => {
                GoType::slice_of(&self.render_recipe_entry(element, mapping)?)
            }
            GoTypeRecipe::Map(key, value) => GoType::map_of(
                &self.render_recipe_entry(key, mapping)?,
                &self.render_recipe_entry(value, mapping)?,
            ),
            GoTypeRecipe::Array(element, length) => GoType::array_of(
                length.to_string(),
                &self.render_recipe_entry(element, mapping)?,
            ),
        })
    }

    fn constructor_go_type(&self, qualified_name: &str, parameters: &[Type], ty: &Type) -> GoType {
        if let Some(index) = self.facts.type_parameter_alias_index(qualified_name)
            && let Some(target) = parameters.get(index)
        {
            return self.go_type(target);
        }

        if let Some(go) = self.anon_struct_go_type(qualified_name) {
            return go;
        }

        let (name, package) = self.unqualify_name(qualified_name);

        let kind = physical_type_kind(ty, |id| self.facts.definition(id));
        let param_types: Vec<GoType> = parameters
            .iter()
            .map(|parameter| self.go_type(parameter))
            .collect();
        if let Some(package) = package {
            let member = name.rsplit('.').next().unwrap_or(&name);
            return GoType::named(member, &param_types, Some(package)).with_physical_kind(kind);
        }
        if let Some(name) = qualified_name.strip_prefix(go_name::PRELUDE_PREFIX)
            && is_prelude_type(name)
        {
            return GoType::prelude(name, &param_types);
        }
        GoType::named(&name, &param_types, None).with_physical_kind(kind)
    }

    fn native_go_type(&self, kind: NativeTypeKind, ty: &Type) -> GoType {
        let argument = |index| {
            let stripped = ty.strip_refs();
            let arguments = stripped.get_type_params().expect("native type arguments");
            self.go_type(&arguments[index])
        };
        match kind {
            NativeTypeKind::String => GoType::new("string"),
            NativeTypeKind::Slice | NativeTypeKind::EnumeratedSlice => {
                GoType::slice_of(&argument(0))
            }
            NativeTypeKind::Map => GoType::map_of(&argument(0), &argument(1)),
            NativeTypeKind::Channel => {
                GoType::channel(ChannelDirection::Bidirectional, &argument(0))
            }
            NativeTypeKind::Sender => GoType::channel(ChannelDirection::Send, &argument(0)),
            NativeTypeKind::Receiver => GoType::channel(ChannelDirection::Receive, &argument(0)),
            NativeTypeKind::Array => self.reject_untyped_position("an array without a length"),
        }
    }

    fn reject_untyped_position(&self, what: &'static str) -> GoType {
        self.namespace.rejections().reject(what, |span| {
            diagnostics::emit::type_without_go_form(what, span)
        });
        GoType::new("any")
    }

    fn unqualify_name(&self, id: &str) -> (String, Option<PackageUse>) {
        if id == "Unknown" {
            return ("any".to_string(), None);
        }
        let (package, unqualified) = if let Some(rest) = id.strip_prefix(go_name::GO_IMPORT_PREFIX)
        {
            let Some((path, ty)) = rest.rsplit_once('.') else {
                return (go_name::escape_keyword(id).into_owned(), None);
            };
            (&id[..go_name::GO_IMPORT_PREFIX.len() + path.len()], ty)
        } else {
            let Some(split) = id.split_once('.') else {
                return (go_name::escape_type_name(id).into_owned(), None);
            };
            split
        };

        if unqualified == "Unknown" {
            return ("any".to_string(), None);
        }

        let escaped = self.facts.type_member_go_name(package, unqualified);

        if self.facts.is_foreign_package(package) {
            if !escaped.starts_with(char::is_uppercase) {
                self.namespace.rejections().reject(id, |span| {
                    let go_type = id.strip_prefix(go_name::GO_IMPORT_PREFIX).unwrap_or(id);
                    diagnostics::emit::unnamable_go_type(go_type, span)
                });
            }
            let package = self.package_use_for_package(package);
            (
                format!("{}.{}", package.qualifier(), escaped),
                Some(package),
            )
        } else {
            (escaped.into_owned(), None)
        }
    }

    pub(crate) fn format_type_args_with_receiver<'t>(
        &self,
        receiver_ty: &Type,
        type_args: impl Iterator<Item = &'t Type>,
    ) -> GoType {
        let receiver_params = receiver_ty.get_type_params().unwrap_or_default();
        let arguments: Vec<GoType> = receiver_params
            .iter()
            .map(|ty| self.go_type(ty))
            .chain(type_args.map(|ty| self.go_type(ty)))
            .collect();
        GoType::type_arguments(&arguments)
    }

    pub(crate) fn return_go_type(&self, abi: &CallableReturnAbi, return_ty: &Type) -> GoType {
        let result_ty = if abi.is_lowered() {
            self.facts.peel_alias(return_ty)
        } else {
            return_ty.clone()
        };
        let result = self.value_layout(&result_ty, SlotOrigin::Lisette);
        self.function_layout(Vec::new(), &result, abi.clone(), None)
            .result_go_type(self)
            .unwrap_or_default()
    }

    pub(crate) fn go_zero(&self, ty: &Type) -> GoExpression {
        self.go_zero_of_layout(&self.value_layout(ty, SlotOrigin::Lisette))
    }

    pub(crate) fn go_zero_of_layout(&self, layout: &ValueLayout) -> GoExpression {
        if self.facts.is_interface(layout.logical_type()) {
            return GoExpression::nil();
        }

        let go_ty = layout.go_type(self);

        let literal = |text: &str| GoExpression::literal(text.to_string());
        let constant = |text: &str, kind| GoExpression::constant(text.to_string(), kind);

        match layout {
            ValueLayout::Plain(Type::Simple(kind)) => match kind {
                SimpleKind::Int
                | SimpleKind::Int8
                | SimpleKind::Int16
                | SimpleKind::Int32
                | SimpleKind::Int64
                | SimpleKind::Uint
                | SimpleKind::Uint8
                | SimpleKind::Uint16
                | SimpleKind::Uint32
                | SimpleKind::Uint64
                | SimpleKind::Uintptr
                | SimpleKind::Byte
                | SimpleKind::Rune => constant("0", ConstantKind::Int),
                SimpleKind::Float32 | SimpleKind::Float64 => constant("0.0", ConstantKind::Float),
                SimpleKind::Bool => constant("false", ConstantKind::Bool),
                SimpleKind::String => constant("\"\"", ConstantKind::String),
                SimpleKind::Unit => GoExpression::unit(),
                SimpleKind::Complex64 | SimpleKind::Complex128 => GoExpression::new_zero(go_ty),
            },
            ValueLayout::Array { .. } => GoExpression::empty_composite(go_ty),
            ValueLayout::Slice { .. }
            | ValueLayout::Map { .. }
            | ValueLayout::Function { .. }
            | ValueLayout::Reference { .. }
            | ValueLayout::NullableOption { .. }
            | ValueLayout::PointerOption { .. }
            | ValueLayout::Plain(Type::Compound {
                kind:
                    CompoundKind::Ref
                    | CompoundKind::Channel
                    | CompoundKind::Sender
                    | CompoundKind::Receiver,
                ..
            }) => literal("nil"),
            ValueLayout::Tuple { .. }
            | ValueLayout::TaggedOption { .. }
            | ValueLayout::Fallible { .. } => GoExpression::empty_composite(go_ty),
            ValueLayout::Plain(Type::Nominal { id, .. })
                if self
                    .facts
                    .definition(id.as_str())
                    .is_some_and(|definition| {
                        matches!(
                            definition.body,
                            DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. }
                        )
                    }) =>
            {
                GoExpression::empty_composite(go_ty)
            }
            _ => GoExpression::new_zero(go_ty),
        }
    }

    fn anon_struct_go_type(&self, id: &str) -> Option<GoType> {
        let definition = self.facts.definition(id)?;
        if !definition.is_anon_struct() {
            return None;
        }
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return None;
        };
        Some(GoType::structure(
            fields
                .iter()
                .map(|field| {
                    (
                        self.facts.declared_field_go_name(id, field),
                        self.go_type(&field.ty),
                    )
                })
                .collect(),
        ))
    }
}

impl ValueLayout {
    pub(crate) fn go_type(&self, planner: &Planner<'_>) -> GoType {
        match self {
            Self::NullableOption { payload, .. } => payload.go_type(planner),
            Self::PointerOption { payload, .. } => GoType::pointer_to(&payload.go_type(planner)),
            Self::Reference { pointee, .. } => GoType::pointer_to(&pointee.go_type(planner)),
            Self::Slice { element, .. } => GoType::slice_of(&element.go_type(planner)),
            Self::Map { key, value, .. } => {
                let key = key.go_type(planner);
                let value = value.go_type(planner);
                GoType::map_of(&key, &value)
            }
            Self::Array {
                length, element, ..
            } => GoType::array_of(length, &element.go_type(planner)),
            Self::Function {
                function_type,
                layout,
            } => {
                if self.function_name().is_some()
                    && self.same_representation(
                        &planner.value_layout(function_type, SlotOrigin::Lisette),
                    )
                {
                    planner.leaf_go_type(function_type)
                } else {
                    layout.go_type(planner)
                }
            }
            Self::TaggedOption { payload, .. } => {
                GoType::prelude("Option", &[payload.go_type(planner)])
            }
            Self::Fallible {
                fallible_type,
                payload,
                error,
            } => {
                let Type::Nominal { id, .. } = fallible_type else {
                    unreachable!("a fallible layout holds a prelude container")
                };
                GoType::prelude(
                    unqualified_name(id),
                    &[payload.go_type(planner), error.go_type(planner)],
                )
            }
            Self::Tuple { elements, .. } => {
                let types: Vec<_> = elements
                    .iter()
                    .map(|element| element.go_type(planner))
                    .collect();
                GoType::prelude(&format!("Tuple{}", elements.len()), &types)
            }
            Self::Named {
                named_type,
                underlying,
                go_alias: true,
            } => planner
                .leaf_go_type(named_type)
                .compared_as(underlying.go_type(planner)),
            Self::Plain(ty) | Self::Named { named_type: ty, .. } => planner.leaf_go_type(ty),
        }
    }
}

impl FunctionLayout {
    pub(crate) fn go_type(&self, planner: &Planner<'_>) -> GoType {
        let parameters: Vec<GoType> = self
            .parameters
            .iter()
            .map(|parameter| parameter.go_type(planner))
            .collect();
        let result = self.result_go_type(planner);
        GoType::function(&parameters, result.as_ref())
    }

    fn result_err_type(&self, planner: &Planner<'_>) -> Type {
        planner.facts.err_payload(self.result_layout.logical_type())
    }

    pub(crate) fn result_go_type(&self, planner: &Planner<'_>) -> Option<GoType> {
        if self.return_abi == CallableReturnAbi::Void {
            return None;
        }
        let mut slots: Vec<GoType> = self
            .results
            .iter()
            .map(|result| result.go_type(planner))
            .collect();
        Some(match self.return_abi.lowered() {
            None
            | Some(LoweredReturnAbi::Option(
                OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_),
            )) => {
                let [slot] = <[GoType; 1]>::try_from(slots)
                    .unwrap_or_else(|_| unreachable!("a single-result ABI has one result slot"));
                slot
            }
            Some(LoweredReturnAbi::BareError) => planner.go_type(&self.result_err_type(planner)),
            Some(LoweredReturnAbi::Result { .. } | LoweredReturnAbi::Partial { .. }) => {
                slots.push(planner.go_type(&self.result_err_type(planner)));
                GoType::result_list(&slots)
            }
            Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })) => {
                slots.push(GoType::new("bool"));
                GoType::result_list(&slots)
            }
            Some(LoweredReturnAbi::Tuple { .. }) => GoType::result_list(&slots),
        })
    }
}

impl EmitFacts<'_> {
    pub(crate) fn type_member_go_name<'n>(&self, package: &str, name: &'n str) -> Cow<'n, str> {
        if package == go_name::PRELUDE_PACKAGE || self.is_foreign_package(package) {
            go_name::escape_keyword(name)
        } else {
            go_name::escape_type_name(name)
        }
    }
}
