use syntax::types::{CompoundKind, Symbol, Type};

use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::{go_result_list, is_prelude_container_type};
use crate::types::go_type::{GoType, returns_go_void};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotOrigin {
    Lisette,
    GoParameter,
    GoReturn,
    GoField,
    GoAny,
}

impl SlotOrigin {
    pub(crate) fn go_parameter(is_any: bool) -> Self {
        Self::go_slot(Self::GoParameter, is_any)
    }

    pub(crate) fn go_return(is_any: bool) -> Self {
        Self::go_slot(Self::GoReturn, is_any)
    }

    pub(crate) fn go_field(is_any: bool) -> Self {
        Self::go_slot(Self::GoField, is_any)
    }

    fn go_slot(origin: Self, is_any: bool) -> Self {
        if is_any { Self::GoAny } else { origin }
    }

    fn nested(self) -> Self {
        if matches!(self, Self::GoAny) {
            Self::Lisette
        } else {
            self
        }
    }

    pub(crate) fn declared_by_go(self) -> bool {
        matches!(self, Self::GoParameter | Self::GoReturn | Self::GoField)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionLayout {
    pub(crate) parameters: Vec<ValueLayout>,
    pub(crate) result_type: Type,
    /// One layout per Go result ahead of the status slot.
    pub(crate) results: Vec<ValueLayout>,
    pub(crate) return_abi: CallableReturnAbi,
}

impl FunctionLayout {
    fn same_representation(&self, other: &Self) -> bool {
        self.return_abi == other.return_abi
            && layouts_match(&self.parameters, &other.parameters)
            && layouts_match(&self.results, &other.results)
    }

    fn go_type(&self, planner: &Planner<'_>) -> GoType {
        let parameters: Vec<GoType> = self
            .parameters
            .iter()
            .map(|parameter| parameter.go_type(planner))
            .collect();
        let result = self.result_go_type(planner);
        let parameter_code = parameters
            .iter()
            .map(|parameter| parameter.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let code = if let Some(result) = &result {
            format!("func({parameter_code}) {}", result.code)
        } else {
            format!("func({parameter_code})")
        };
        let mut go_type = GoType::with_dependencies(code, &parameters);
        if let Some(result) = &result {
            go_type.merge(result);
        }
        go_type
    }

    pub(crate) fn result_go_type(&self, planner: &Planner<'_>) -> Option<GoType> {
        if returns_go_void(&self.result_type) {
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
            Some(LoweredReturnAbi::BareError) => planner.go_type(&self.result_type.err_type()),
            Some(LoweredReturnAbi::Result { .. } | LoweredReturnAbi::Partial { .. }) => {
                slots.push(planner.go_type(&self.result_type.err_type()));
                go_result_list(&slots)
            }
            Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })) => {
                slots.push(GoType::new("bool"));
                go_result_list(&slots)
            }
            Some(LoweredReturnAbi::Tuple { .. }) => go_result_list(&slots),
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ValueLayout {
    Plain(Type),
    Fallible {
        fallible_type: Type,
        payload: Box<ValueLayout>,
    },
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
    pub(crate) fn payload(&self) -> Option<&Self> {
        match self {
            Self::Fallible { payload, .. }
            | Self::TaggedOption { payload, .. }
            | Self::NullableOption { payload, .. }
            | Self::PointerOption { payload, .. } => Some(payload),
            Self::Named { underlying, .. } => underlying.payload(),
            _ => None,
        }
    }

    pub(crate) fn tuple_elements(&self) -> Option<&[Self]> {
        match self {
            Self::Tuple { elements, .. } => Some(elements),
            Self::Named { underlying, .. } => underlying.tuple_elements(),
            _ => None,
        }
    }

    pub(crate) fn same_representation(&self, other: &Self) -> bool {
        match (self, other) {
            // A `Result` is always the tagged struct, whatever its payload layout.
            (Self::Plain(_), Self::Plain(_)) | (Self::Fallible { .. }, Self::Fallible { .. }) => {
                true
            }
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
                left.same_representation(right)
            }
            (
                Self::Tuple { elements: left, .. },
                Self::Tuple {
                    elements: right, ..
                },
            ) => layouts_match(left, right),
            _ => false,
        }
    }

    pub(crate) fn logical_type(&self) -> &Type {
        match self {
            Self::Plain(ty)
            | Self::Fallible {
                fallible_type: ty, ..
            }
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

    pub(crate) fn go_type(&self, planner: &Planner<'_>) -> GoType {
        match self {
            Self::NullableOption { payload, .. } => payload.go_type(planner),
            Self::PointerOption { payload, .. } => derived_go_type("*", payload.go_type(planner)),
            Self::Reference { pointee, .. } => derived_go_type("*", pointee.go_type(planner)),
            Self::Slice { element, .. } => derived_go_type("[]", element.go_type(planner)),
            Self::Map { key, value, .. } => {
                let key = key.go_type(planner);
                let value = value.go_type(planner);
                GoType::with_dependencies(
                    format!("map[{}]{}", key.code, value.code),
                    [&key, &value],
                )
            }
            Self::Array {
                length, element, ..
            } => derived_go_type(&format!("[{length}]"), element.go_type(planner)),
            Self::Function { layout, .. } => layout.go_type(planner),
            Self::Plain(ty)
            | Self::Fallible {
                fallible_type: ty, ..
            }
            | Self::TaggedOption {
                option_type: ty, ..
            }
            | Self::Tuple { tuple_type: ty, .. }
            | Self::Named { named_type: ty, .. } => planner.go_type(ty),
        }
    }
}

fn derived_go_type(prefix: &str, inner: GoType) -> GoType {
    GoType::with_dependencies(format!("{prefix}{}", inner.code), [&inner])
}

impl Planner<'_> {
    pub(crate) fn field_slot_layout(
        &self,
        owner_type: &Type,
        declaring_type: Option<&Symbol>,
        field: &str,
        value_type: &Type,
    ) -> Option<ValueLayout> {
        let owner = match declaring_type {
            Some(declaring) => declaring.clone(),
            None => self.resolve_nominal(owner_type)?.id,
        };
        let slot = self.facts.go_field(owner.as_str(), field)?;
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

    pub(crate) fn function_layout(
        &self,
        parameters: Vec<ValueLayout>,
        result: &ValueLayout,
        return_abi: CallableReturnAbi,
    ) -> FunctionLayout {
        let payload = || {
            result
                .payload()
                .expect("a payload-carrying return ABI has a payload layout")
        };
        let results = match return_abi.lowered() {
            None => vec![result.clone()],
            Some(LoweredReturnAbi::BareError) => Vec::new(),
            Some(LoweredReturnAbi::Option(
                OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_),
            )) => vec![payload().clone()],
            Some(
                lowered @ (LoweredReturnAbi::Result { .. }
                | LoweredReturnAbi::Partial { .. }
                | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })),
            ) => {
                if lowered.has_flattened_payload() {
                    self.tuple_result_slots(payload())
                } else {
                    vec![payload().clone()]
                }
            }
            Some(LoweredReturnAbi::Tuple { .. }) => self.tuple_result_slots(result),
        };
        FunctionLayout {
            parameters,
            result_type: result.logical_type().clone(),
            results,
            return_abi,
        }
    }

    fn tuple_result_slots(&self, tuple: &ValueLayout) -> Vec<ValueLayout> {
        tuple
            .tuple_elements()
            .expect("a tuple-shaped return ABI has a tuple layout")
            .iter()
            .map(|element| match element {
                ValueLayout::TaggedOption {
                    option_type,
                    payload,
                } if self.facts.is_nullable_option(option_type) => ValueLayout::NullableOption {
                    option_type: option_type.clone(),
                    payload: payload.clone(),
                },
                element => element.clone(),
            })
            .collect()
    }

    fn value_layout_with_hint(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> ValueLayout {
        let resolved_declaration = declaration.map(|ty| self.facts.peel_alias(ty));
        if resolved_declaration.as_ref().is_some_and(|ty| {
            self.facts.resolves_to_unknown(ty)
                || matches!(
                    ty.unwrap_forall(),
                    Type::Parameter(_) | Type::Var { .. } | Type::ReceiverPlaceholder
                )
        }) {
            return self.value_layout_with_hint(ty, SlotOrigin::Lisette, None);
        }

        let origin = self.function_type_origin(ty, origin);
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
        if is_prelude_container_type(&ty) {
            let declared_payload = declaration
                .filter(|ty| is_prelude_container_type(ty))
                .map(Type::ok_type);
            let payload = Box::new(self.value_layout_with_hint(
                &ty.ok_type(),
                origin.nested(),
                declared_payload.as_ref(),
            ));
            if !ty.is_option() {
                return ValueLayout::Fallible {
                    fallible_type: ty,
                    payload,
                };
            }
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
                ..
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
                ..
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
                ..
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
                                &parameter.ty,
                                origin.nested(),
                                declared_parameters.get(index).map(|param| &param.ty),
                            )
                        })
                        .collect();
                    let result_type = function_type
                        .get_function_ret()
                        .cloned()
                        .unwrap_or(Type::Never);
                    let declared_result =
                        declared_function.as_ref().and_then(Type::get_function_ret);
                    let result =
                        self.value_layout_with_hint(&result_type, origin.nested(), declared_result);
                    let return_abi = self.slot_return_abi(&result_type, origin);
                    return ValueLayout::Function {
                        function_type: ty,
                        layout: self.function_layout(parameters, &result, return_abi),
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

fn layouts_match(left: &[ValueLayout], right: &[ValueLayout]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.same_representation(right))
}

fn compound_hint(
    declaration: Option<&Type>,
    expected_kind: CompoundKind,
    index: usize,
) -> Option<&Type> {
    let Type::Compound { kind, args, .. } = declaration? else {
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
