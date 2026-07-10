use syntax::types::{CompoundKind, Type};

use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotOrigin {
    Lisette,
    GoParameter,
    GoReturn,
    GoField,
    GoAny,
}

impl SlotOrigin {
    pub(crate) fn go_parameter(ty: &Type) -> Self {
        Self::go_slot(Self::GoParameter, ty)
    }

    pub(crate) fn go_return(ty: &Type) -> Self {
        Self::go_slot(Self::GoReturn, ty)
    }

    pub(crate) fn go_field(ty: &Type) -> Self {
        Self::go_slot(Self::GoField, ty)
    }

    fn go_slot(origin: Self, ty: &Type) -> Self {
        if ty.resolves_to_unknown() {
            Self::GoAny
        } else {
            origin
        }
    }

    fn nested(self) -> Self {
        if matches!(self, Self::GoAny) {
            Self::Lisette
        } else {
            self
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionLayout {
    pub(crate) parameters: Vec<ValueLayout>,
    pub(crate) result: Box<ValueLayout>,
    pub(crate) payload: Option<Box<ValueLayout>>,
    pub(crate) return_abi: CallableReturnAbi,
}

impl FunctionLayout {
    fn result_same_representation(&self, other: &Self) -> bool {
        match self.return_abi {
            CallableReturnAbi::Result { .. }
            | CallableReturnAbi::Partial { .. }
            | CallableReturnAbi::Option { .. } => {
                optional_layouts_match(self.payload.as_deref(), other.payload.as_deref())
            }
            CallableReturnAbi::Tagged
            | CallableReturnAbi::Direct
            | CallableReturnAbi::Tuple { .. } => self.result.same_representation(&other.result),
        }
    }

    pub(crate) fn go_type(&self, planner: &Planner<'_>) -> String {
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| parameter.go_type(planner))
            .collect::<Vec<_>>()
            .join(", ");
        let result = self.result_go_type(planner);
        if result.is_empty() {
            format!("func({parameters})")
        } else {
            format!("func({parameters}) {result}")
        }
    }

    pub(crate) fn result_go_type(&self, planner: &Planner<'_>) -> String {
        if self.result.logical_type().is_unit() {
            return String::new();
        }
        match &self.return_abi {
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct => self.result.go_type(planner),
            CallableReturnAbi::Result {
                bare_error: true, ..
            } => planner.go_type_string(&self.result.logical_type().err_type()),
            CallableReturnAbi::Result { .. } | CallableReturnAbi::Partial { .. } => {
                let payload = self
                    .payload
                    .as_deref()
                    .expect("fallible callable layout has a payload");
                let error = planner.go_type_string(&self.result.logical_type().err_type());
                format!("({}, {error})", payload.go_type(planner))
            }
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::CommaOk,
                ..
            } => {
                let payload = self
                    .payload
                    .as_deref()
                    .expect("option callable layout has a payload");
                format!("({}, bool)", payload.go_type(planner))
            }
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::Nullable,
                ..
            } => self
                .payload
                .as_deref()
                .expect("option callable layout has a payload")
                .go_type(planner),
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::Sentinel(_),
                ..
            } => self
                .payload
                .as_deref()
                .expect("option callable layout has a payload")
                .go_type(planner),
            CallableReturnAbi::Tuple { .. } => {
                let ValueLayout::Tuple { elements, .. } = self.result.as_ref() else {
                    return self.result.go_type(planner);
                };
                format!(
                    "({})",
                    elements
                        .iter()
                        .map(|element| element.go_type(planner))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ValueLayout {
    Plain(Type),
    TaggedOption {
        option_type: Type,
        payload: Box<ValueLayout>,
    },
    NullableOption {
        option_type: Type,
        payload: Box<ValueLayout>,
    },
    PointerOption {
        option_type: Type,
        payload: Box<ValueLayout>,
    },
    Reference {
        reference_type: Type,
        pointee: Box<ValueLayout>,
    },
    Slice {
        collection_type: Type,
        element: Box<ValueLayout>,
    },
    Map {
        collection_type: Type,
        key: Box<ValueLayout>,
        value: Box<ValueLayout>,
    },
    Array {
        array_type: Type,
        length: u64,
        element: Box<ValueLayout>,
    },
    Function {
        function_type: Type,
        layout: FunctionLayout,
    },
    Tuple {
        tuple_type: Type,
        elements: Vec<ValueLayout>,
    },
    Named {
        named_type: Type,
        underlying: Box<ValueLayout>,
    },
}

impl ValueLayout {
    pub(crate) fn option_payload(&self) -> Option<&Self> {
        match self {
            Self::TaggedOption { payload, .. }
            | Self::NullableOption { payload, .. }
            | Self::PointerOption { payload, .. } => Some(payload),
            Self::Named { underlying, .. } => underlying.option_payload(),
            _ => None,
        }
    }

    pub(crate) fn same_representation(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Plain(_), Self::Plain(_)) => true,
            (
                Self::TaggedOption { payload: left, .. },
                Self::TaggedOption { payload: right, .. },
            )
            | (
                Self::NullableOption { payload: left, .. },
                Self::NullableOption { payload: right, .. },
            )
            | (
                Self::PointerOption { payload: left, .. },
                Self::PointerOption { payload: right, .. },
            )
            | (Self::Reference { pointee: left, .. }, Self::Reference { pointee: right, .. })
            | (Self::Slice { element: left, .. }, Self::Slice { element: right, .. })
            | (
                Self::Named {
                    underlying: left, ..
                },
                Self::Named {
                    underlying: right, ..
                },
            ) => left.same_representation(right),
            (
                Self::Array {
                    length: left_length,
                    element: left,
                    ..
                },
                Self::Array {
                    length: right_length,
                    element: right,
                    ..
                },
            ) => left_length == right_length && left.same_representation(right),
            (
                Self::Map {
                    key: left_key,
                    value: left_value,
                    ..
                },
                Self::Map {
                    key: right_key,
                    value: right_value,
                    ..
                },
            ) => {
                left_key.same_representation(right_key)
                    && left_value.same_representation(right_value)
            }
            (Self::Function { layout: left, .. }, Self::Function { layout: right, .. }) => {
                left.return_abi == right.return_abi
                    && left.parameters.len() == right.parameters.len()
                    && left
                        .parameters
                        .iter()
                        .zip(&right.parameters)
                        .all(|(left, right)| left.same_representation(right))
                    && left.result_same_representation(right)
            }
            (
                Self::Tuple { elements: left, .. },
                Self::Tuple {
                    elements: right, ..
                },
            ) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| left.same_representation(right))
            }
            _ => false,
        }
    }

    pub(crate) fn logical_type(&self) -> &Type {
        match self {
            Self::Plain(ty)
            | Self::TaggedOption {
                option_type: ty, ..
            }
            | Self::NullableOption {
                option_type: ty, ..
            }
            | Self::PointerOption {
                option_type: ty, ..
            }
            | Self::Reference {
                reference_type: ty, ..
            }
            | Self::Slice {
                collection_type: ty,
                ..
            }
            | Self::Map {
                collection_type: ty,
                ..
            }
            | Self::Array { array_type: ty, .. }
            | Self::Function {
                function_type: ty, ..
            }
            | Self::Tuple { tuple_type: ty, .. }
            | Self::Named { named_type: ty, .. } => ty,
        }
    }

    pub(crate) fn go_type(&self, planner: &Planner<'_>) -> String {
        match self {
            Self::NullableOption { payload, .. } => payload.go_type(planner),
            Self::PointerOption { payload, .. } => format!("*{}", payload.go_type(planner)),
            Self::Reference { pointee, .. } => format!("*{}", pointee.go_type(planner)),
            Self::Slice { element, .. } => format!("[]{}", element.go_type(planner)),
            Self::Map { key, value, .. } => {
                format!("map[{}]{}", key.go_type(planner), value.go_type(planner))
            }
            Self::Array {
                length, element, ..
            } => format!("[{length}]{}", element.go_type(planner)),
            Self::Function { layout, .. } => layout.go_type(planner),
            Self::Plain(ty)
            | Self::TaggedOption {
                option_type: ty, ..
            }
            | Self::Tuple { tuple_type: ty, .. }
            | Self::Named { named_type: ty, .. } => planner.go_type_string(ty),
        }
    }
}

impl Planner<'_> {
    pub(crate) fn field_slot_layout(
        &self,
        owner_type: &Type,
        field: &str,
        value_type: &Type,
    ) -> Option<ValueLayout> {
        let owner = self.resolve_nominal(owner_type)?;
        let slot = self.facts.go_field(owner.id.as_str(), field)?;
        Some(self.value_layout_with_declaration(value_type, slot.origin, &slot.declared_type))
    }

    pub(crate) fn is_go_abi_type(&self, ty: &Type) -> bool {
        self.resolve_nominal(ty)
            .is_some_and(|resolved| self.facts.is_go_imported_type(resolved.id.as_str()))
    }

    pub(crate) fn value_layout(&self, ty: &Type, origin: SlotOrigin) -> ValueLayout {
        self.value_layout_with_hint(ty, origin, None)
    }

    pub(crate) fn value_layout_with_declaration(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: &Type,
    ) -> ValueLayout {
        self.value_layout_with_hint(ty, origin, Some(declaration))
    }

    pub(crate) fn callable_payload_layout(
        &self,
        result_type: &Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> Option<ValueLayout> {
        let payload = callable_payload_type(result_type)?;
        let declared_payload = declaration.and_then(callable_payload_type);
        Some(self.value_layout_with_hint(&payload, origin.nested(), declared_payload.as_ref()))
    }

    fn value_layout_with_hint(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> ValueLayout {
        let resolved_declaration = declaration.map(|ty| self.facts.peel_alias(ty));
        if resolved_declaration
            .as_ref()
            .is_some_and(is_opaque_layout_hint)
        {
            return self.value_layout_with_hint(ty, SlotOrigin::Lisette, None);
        }

        let resolved = self.facts.peel_alias(ty);
        if resolved != *ty {
            return ValueLayout::Named {
                named_type: ty.clone(),
                underlying: Box::new(self.value_layout_resolved(
                    resolved,
                    origin,
                    resolved_declaration.as_ref(),
                )),
            };
        }
        self.value_layout_resolved(resolved, origin, resolved_declaration.as_ref())
    }

    fn value_layout_resolved(
        &self,
        ty: Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> ValueLayout {
        if ty.is_option() {
            let declared_payload = declaration.filter(|ty| ty.is_option()).map(Type::ok_type);
            let payload = Box::new(self.value_layout_with_hint(
                &ty.ok_type(),
                origin.nested(),
                declared_payload.as_ref(),
            ));
            return match origin {
                SlotOrigin::Lisette | SlotOrigin::GoAny => ValueLayout::TaggedOption {
                    option_type: ty,
                    payload,
                },
                SlotOrigin::GoParameter | SlotOrigin::GoField
                    if self.facts.is_nullable_option(&ty) =>
                {
                    ValueLayout::NullableOption {
                        option_type: ty,
                        payload,
                    }
                }
                SlotOrigin::GoParameter | SlotOrigin::GoReturn | SlotOrigin::GoField
                    if self.is_non_nilable_option(&ty) =>
                {
                    ValueLayout::PointerOption {
                        option_type: ty,
                        payload,
                    }
                }
                SlotOrigin::GoReturn if self.facts.is_nullable_option(&ty) => {
                    ValueLayout::NullableOption {
                        option_type: ty,
                        payload,
                    }
                }
                SlotOrigin::GoParameter | SlotOrigin::GoReturn | SlotOrigin::GoField => {
                    ValueLayout::TaggedOption {
                        option_type: ty,
                        payload,
                    }
                }
            };
        }

        match &ty {
            Type::Compound {
                kind: CompoundKind::Ref,
                args,
            } => {
                let Some(pointee) = args.first() else {
                    return ValueLayout::Plain(ty);
                };
                ValueLayout::Reference {
                    reference_type: ty.clone(),
                    pointee: Box::new(self.value_layout_with_hint(
                        pointee,
                        origin.nested(),
                        compound_hint(declaration, CompoundKind::Ref, 0),
                    )),
                }
            }
            Type::Compound {
                kind: kind @ (CompoundKind::Slice | CompoundKind::EnumeratedSlice),
                args,
            } => {
                let Some(element) = args.first() else {
                    return ValueLayout::Plain(ty);
                };
                ValueLayout::Slice {
                    collection_type: ty.clone(),
                    element: Box::new(self.value_layout_with_hint(
                        element,
                        origin.nested(),
                        compound_hint(declaration, *kind, 0),
                    )),
                }
            }
            Type::Compound {
                kind: CompoundKind::Map,
                args,
            } => {
                let [key, value] = args.as_slice() else {
                    return ValueLayout::Plain(ty);
                };
                ValueLayout::Map {
                    collection_type: ty.clone(),
                    key: Box::new(self.value_layout_with_hint(
                        key,
                        origin.nested(),
                        compound_hint(declaration, CompoundKind::Map, 0),
                    )),
                    value: Box::new(self.value_layout_with_hint(
                        value,
                        origin.nested(),
                        compound_hint(declaration, CompoundKind::Map, 1),
                    )),
                }
            }
            Type::Array { length, element } => ValueLayout::Array {
                array_type: ty.clone(),
                length: *length,
                element: Box::new(self.value_layout_with_hint(
                    element,
                    origin.nested(),
                    array_hint(declaration),
                )),
            },
            Type::Tuple(elements) => ValueLayout::Tuple {
                tuple_type: ty.clone(),
                elements: elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        self.value_layout_with_hint(
                            element,
                            origin.nested(),
                            tuple_hint(declaration, index),
                        )
                    })
                    .collect(),
            },
            _ => {
                if let Some(function_type) = self.facts.resolve_to_function_type(&ty) {
                    let declared_function =
                        declaration.and_then(|ty| self.facts.resolve_to_function_type(ty));
                    let declared_parameters = declared_function
                        .as_ref()
                        .and_then(Type::get_function_params)
                        .unwrap_or_default();
                    let parameters = function_type
                        .get_function_params()
                        .unwrap_or_default()
                        .iter()
                        .enumerate()
                        .map(|(index, parameter)| {
                            self.value_layout_with_hint(
                                parameter,
                                origin.nested(),
                                declared_parameters.get(index),
                            )
                        })
                        .collect();
                    let result_type = function_type
                        .get_function_ret()
                        .cloned()
                        .unwrap_or(Type::Never);
                    let declared_result =
                        declared_function.as_ref().and_then(Type::get_function_ret);
                    let result = Box::new(self.value_layout_with_hint(
                        &result_type,
                        origin.nested(),
                        declared_result,
                    ));
                    let payload = self
                        .callable_payload_layout(&result_type, origin, declared_result)
                        .map(Box::new);
                    let return_abi = self.callable_return_abi(&result_type);
                    return ValueLayout::Function {
                        function_type: ty,
                        layout: FunctionLayout {
                            parameters,
                            result,
                            payload,
                            return_abi,
                        },
                    };
                }
                if let Some(underlying) = self.get_newtype_underlying(&ty) {
                    let declared_underlying =
                        declaration.and_then(|ty| self.get_newtype_underlying(ty));
                    return ValueLayout::Named {
                        named_type: ty,
                        underlying: Box::new(self.value_layout_with_hint(
                            &underlying,
                            origin.nested(),
                            declared_underlying.as_ref(),
                        )),
                    };
                }
                ValueLayout::Plain(ty)
            }
        }
    }
}

fn callable_payload_type(ty: &Type) -> Option<Type> {
    (ty.is_result() || ty.is_partial() || ty.is_option()).then(|| ty.ok_type())
}

fn optional_layouts_match(left: Option<&ValueLayout>, right: Option<&ValueLayout>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.same_representation(right),
        (None, None) => true,
        _ => false,
    }
}

fn is_opaque_layout_hint(ty: &Type) -> bool {
    ty.resolves_to_unknown()
        || matches!(
            ty.unwrap_forall(),
            Type::Parameter(_) | Type::Var { .. } | Type::ReceiverPlaceholder
        )
}

fn compound_hint(
    declaration: Option<&Type>,
    expected_kind: CompoundKind,
    index: usize,
) -> Option<&Type> {
    let Type::Compound { kind, args } = declaration? else {
        return None;
    };
    (*kind == expected_kind).then(|| args.get(index)).flatten()
}

fn array_hint(declaration: Option<&Type>) -> Option<&Type> {
    let Type::Array { element, .. } = declaration? else {
        return None;
    };
    Some(element)
}

fn tuple_hint(declaration: Option<&Type>, index: usize) -> Option<&Type> {
    let Type::Tuple(elements) = declaration? else {
        return None;
    };
    elements.get(index)
}
