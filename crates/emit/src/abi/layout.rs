use syntax::ast::Expression;
use syntax::program::DotAccessResolution;
use syntax::types::{CompoundKind, Symbol, Type};

use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::{go_result_list, is_prelude_container_type, tuple_element_types};
use crate::names::go_name;
use crate::types::go_type::GoType;

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
    pub(crate) fn same_representation(&self, other: &Self) -> bool {
        self.same_parameter_representation(other) && self.same_result_representation(other)
    }

    pub(crate) fn same_parameter_representation(&self, other: &Self) -> bool {
        layouts_match(&self.parameters, &other.parameters)
    }

    pub(crate) fn same_result_representation(&self, other: &Self) -> bool {
        self.return_abi == other.return_abi && layouts_match(&self.results, &other.results)
    }

    pub(crate) fn go_type(&self, planner: &Planner<'_>) -> GoType {
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
        error: Box<ValueLayout>,
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
    pub(crate) fn underlying(&self) -> &Self {
        match self {
            Self::Named { underlying, .. } => underlying.underlying(),
            _ => self,
        }
    }

    pub(crate) fn payload(&self) -> Option<&Self> {
        match self.underlying() {
            Self::Fallible { payload, .. }
            | Self::TaggedOption { payload, .. }
            | Self::NullableOption { payload, .. }
            | Self::PointerOption { payload, .. } => Some(payload),
            _ => None,
        }
    }

    pub(crate) fn tuple_elements(&self) -> Option<&[Self]> {
        match self.underlying() {
            Self::Tuple { elements, .. } => Some(elements),
            _ => None,
        }
    }

    pub(crate) fn same_representation(&self, other: &Self) -> bool {
        if self.function_name() != other.function_name() {
            return false;
        }
        match (self, other) {
            (Self::Plain(_), Self::Plain(_)) => true,
            (
                Self::Fallible {
                    payload: left,
                    error: left_error,
                    ..
                },
                Self::Fallible {
                    payload: right,
                    error: right_error,
                    ..
                },
            ) => left.same_representation(right) && left_error.same_representation(right_error),
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

    pub(crate) fn function_name(&self) -> Option<&Type> {
        match self {
            Self::Named {
                named_type,
                underlying,
            } if matches!(underlying.underlying(), Self::Function { .. }) => Some(named_type),
            Self::Function { function_type, .. }
                if matches!(function_type.unwrap_forall(), Type::Nominal { .. }) =>
            {
                Some(function_type)
            }
            _ => None,
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
            Self::TaggedOption { payload, .. } => {
                container_go_type("Option", &[payload.go_type(planner)])
            }
            Self::Fallible {
                fallible_type,
                payload,
                error,
            } => {
                let name = if fallible_type.is_partial() {
                    "Partial"
                } else {
                    "Result"
                };
                container_go_type(name, &[payload.go_type(planner), error.go_type(planner)])
            }
            Self::Tuple { elements, .. } => {
                let types: Vec<_> = elements
                    .iter()
                    .map(|element| element.go_type(planner))
                    .collect();
                container_go_type(&format!("Tuple{}", elements.len()), &types)
            }
            Self::Plain(ty) | Self::Named { named_type: ty, .. } => planner.go_type(ty),
        }
    }
}

fn container_go_type(name: &str, elements: &[GoType]) -> GoType {
    let arguments = elements
        .iter()
        .map(|element| element.code.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut ty = GoType::stdlib(format!("{}.{name}[{arguments}]", go_name::GO_STDLIB_PKG));
    for element in elements {
        ty.merge(element);
    }
    ty
}

fn derived_go_type(prefix: &str, inner: GoType) -> GoType {
    GoType::with_dependencies(format!("{prefix}{}", inner.code), [&inner])
}

impl Planner<'_> {
    /// A prelude method uses the type arguments of its receiver's stored Go
    /// representation. Adapting the receiver itself would copy mutable storage.
    pub(crate) fn prelude_receiver_method_layouts(
        &self,
        expression: &Expression,
    ) -> Option<(FunctionLayout, FunctionLayout)> {
        let Expression::DotAccess {
            expression: receiver,
            resolution: DotAccessResolution::InstanceMethod { .. },
            ..
        } = expression
        else {
            return None;
        };
        let callee = self.resolve_callable_value(expression)?;
        if !callee.is_prelude_dispatch {
            return None;
        }
        let declaration = callee.declared_type()?.as_function_type()?;
        let (receiver_param, parameters) = declaration.params.split_first()?;
        let receiver_declaration = receiver_param.ty.strip_refs();
        let receiver_layout =
            self.value_layout(&receiver.get_type().strip_refs(), SlotOrigin::Lisette);
        let slots = match receiver_layout.underlying() {
            ValueLayout::TaggedOption { payload, .. } => vec![payload.as_ref()],
            ValueLayout::Fallible { payload, error, .. } => vec![payload.as_ref(), error.as_ref()],
            _ => return None,
        };
        let bindings: Vec<_> = receiver_declaration
            .get_type_params()?
            .iter()
            .zip(slots)
            .collect();
        let mut target = callee.abi.function_layout(self);
        let instantiated = callee.instantiated.as_function_type()?;
        let canonical_receiver = self.value_layout(
            &self.facts.strip_and_peel(&receiver.get_type()),
            SlotOrigin::Lisette,
        );
        let canonical_slots = match canonical_receiver.underlying() {
            ValueLayout::TaggedOption { payload, .. } => vec![payload.as_ref()],
            ValueLayout::Fallible { payload, error, .. } => vec![payload.as_ref(), error.as_ref()],
            _ => unreachable!("prelude receiver has the same container after alias expansion"),
        };
        let canonical_bindings: Vec<_> = receiver_declaration
            .get_type_params()?
            .iter()
            .zip(canonical_slots)
            .collect();
        for ((slot, declared), actual) in target
            .parameters
            .iter_mut()
            .zip(parameters)
            .zip(&instantiated.params)
        {
            if declared.ty.as_function_type().is_some() {
                *slot =
                    self.layout_with_receiver_slots(&actual.ty, &declared.ty, &canonical_bindings);
            }
        }
        let parameters = parameters
            .iter()
            .zip(&instantiated.params)
            .map(|(declared, actual)| {
                self.layout_with_receiver_slots(&actual.ty, &declared.ty, &bindings)
            })
            .collect();
        let result = self.layout_with_receiver_slots(
            &instantiated.return_type,
            &declaration.return_type,
            &bindings,
        );
        let source = self.function_layout(
            parameters,
            &result,
            target.return_abi.clone(),
            Some(&declaration.return_type),
        );
        (!source.same_representation(&target)).then_some((source, target))
    }

    fn layout_with_receiver_slots(
        &self,
        ty: &Type,
        declaration: &Type,
        bindings: &[(&Type, &ValueLayout)],
    ) -> ValueLayout {
        if let Some((_, slot)) = bindings
            .iter()
            .find(|(parameter, _)| *parameter == declaration)
        {
            return (*slot).clone();
        }
        let mut layout = self.value_layout_with_declaration(ty, SlotOrigin::Lisette, declaration);
        let declared = self.facts.peel_alias(declaration);
        let actual = self.facts.peel_alias(ty);
        let mut inner = &mut layout;
        while let ValueLayout::Named { underlying, .. } = inner {
            inner = underlying;
        }
        match inner {
            ValueLayout::TaggedOption { payload, .. } if declared.is_option() => {
                **payload = self.layout_with_receiver_slots(
                    &actual.ok_type(),
                    &declared.ok_type(),
                    bindings,
                );
            }
            ValueLayout::Fallible { payload, error, .. }
                if declared.is_result() || declared.is_partial() =>
            {
                **payload = self.layout_with_receiver_slots(
                    &actual.ok_type(),
                    &declared.ok_type(),
                    bindings,
                );
                **error = self.layout_with_receiver_slots(
                    &actual.err_type(),
                    &declared.err_type(),
                    bindings,
                );
            }
            ValueLayout::Function { layout, .. } => {
                let declared = declared
                    .as_function_type()
                    .expect("function slot declaration");
                let actual = actual
                    .as_function_type()
                    .expect("function slot instantiation");
                let parameters = actual
                    .params
                    .iter()
                    .zip(&declared.params)
                    .map(|(actual, declared)| {
                        self.layout_with_receiver_slots(&actual.ty, &declared.ty, bindings)
                    })
                    .collect();
                let result = self.layout_with_receiver_slots(
                    &actual.return_type,
                    &declared.return_type,
                    bindings,
                );
                *layout = self.function_layout(
                    parameters,
                    &result,
                    CallableReturnAbi::unlowered(&declared.return_type),
                    Some(&declared.return_type),
                );
            }
            _ => {}
        }
        layout
    }

    pub(crate) fn field_slot_layout(
        &self,
        owner_type: &Type,
        declaring_type: Option<&Symbol>,
        variant: Option<&str>,
        field: &str,
        value_type: &Type,
    ) -> Option<ValueLayout> {
        if let Ok(index) = field.parse::<usize>()
            && let Some(elements) = self
                .value_layout(&owner_type.strip_refs(), SlotOrigin::Lisette)
                .tuple_elements()
        {
            return elements.get(index).cloned();
        }
        let owner = match declaring_type {
            Some(declaring) => declaring.clone(),
            None => self.resolve_nominal(owner_type)?.id,
        };
        if let Some(slot) = self.facts.go_field(owner.as_str(), field) {
            return Some(self.value_layout_with_declaration(
                value_type,
                slot.origin,
                &slot.declared_type,
            ));
        }
        let function_type = self.facts.resolve_to_function_type(value_type);
        let (declared, _) = self.declared_field_ty(owner_type, variant, field)?;
        let declaration = self
            .facts
            .function_alias_declaration(value_type)
            .unwrap_or(declared);
        let layout = self.value_layout_with_declaration(
            function_type.as_ref().unwrap_or(value_type),
            self.function_type_origin(value_type, SlotOrigin::Lisette),
            &declaration,
        );
        Some(layout)
    }

    pub(crate) fn is_go_abi_type(&self, ty: &Type) -> bool {
        self.resolve_nominal(ty)
            .is_some_and(|resolved| self.facts.is_go_imported_type(resolved.id.as_str()))
    }

    pub(crate) fn value_layout(&self, ty: &Type, origin: SlotOrigin) -> ValueLayout {
        self.value_layout_with_hint(ty, origin, None)
    }

    pub(crate) fn callable_result_layout(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
        abi: &CallableReturnAbi,
    ) -> ValueLayout {
        // Lowered signatures render the alias's expanded result slots. Stored
        // aliases still use the physical slots of their own declaration.
        let resolved = abi.lowered().map(|_| self.facts.peel_alias(ty));
        let declared = abi
            .lowered()
            .and_then(|_| declaration.map(|ty| self.facts.peel_alias(ty)));
        let result = resolved.as_ref().unwrap_or(ty);
        let layout = declared.as_ref().or(declaration).map_or_else(
            || self.value_layout(result, origin),
            |declaration| self.value_layout_with_declaration(result, origin, declaration),
        );
        if result != ty {
            ValueLayout::Named {
                named_type: ty.clone(),
                underlying: Box::new(layout),
            }
        } else {
            layout
        }
    }

    pub(crate) fn value_layout_with_declaration(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: &Type,
    ) -> ValueLayout {
        if matches!(ty, Type::Function(_))
            && let Some(declaration) = self.facts.resolve_to_function_type(declaration)
        {
            return self.value_layout_resolved(
                ty.clone(),
                origin,
                Some(&declaration),
                declaration.get_function_ret(),
            );
        }
        if self.facts.resolve_to_function_type(declaration).is_none()
            && let Some(declaration) = self.facts.alias_declaration(declaration)
        {
            return self.value_layout_with_declaration(ty, origin, declaration);
        }
        let mut layout = self.value_layout_with_hint(ty, origin, Some(declaration));
        let declaration = self.facts.peel_alias(declaration);
        // Immutable containers preserve each declared function slot. Reference and
        // collection storage requires a shared representation, not a copied bridge.
        match &mut layout {
            ValueLayout::TaggedOption { payload, .. }
            | ValueLayout::NullableOption { payload, .. }
            | ValueLayout::PointerOption { payload, .. }
                if is_prelude_container_type(&declaration) =>
            {
                **payload = self.value_layout_with_declaration(
                    &ty.ok_type(),
                    origin.nested(),
                    &declaration.ok_type(),
                );
            }
            ValueLayout::Fallible { payload, error, .. }
                if is_prelude_container_type(&declaration) =>
            {
                **payload = self.value_layout_with_declaration(
                    &ty.ok_type(),
                    origin.nested(),
                    &declaration.ok_type(),
                );
                **error = self.value_layout_with_declaration(
                    &ty.err_type(),
                    origin.nested(),
                    &declaration.err_type(),
                );
            }
            ValueLayout::Tuple { elements, .. } => {
                for (index, element) in elements.iter_mut().enumerate() {
                    if let Some(declared) = tuple_hint(Some(&declaration), index) {
                        *element = self.value_layout_with_declaration(
                            element.logical_type(),
                            origin.nested(),
                            declared,
                        );
                    }
                }
            }
            _ => {}
        }
        layout
    }

    pub(crate) fn function_layout(
        &self,
        parameters: Vec<ValueLayout>,
        result: &ValueLayout,
        return_abi: CallableReturnAbi,
        declaration: Option<&Type>,
    ) -> FunctionLayout {
        let payload = || {
            result
                .payload()
                .expect("a payload-carrying return ABI has a payload layout")
        };
        let results = match return_abi.lowered() {
            None if return_abi == CallableReturnAbi::Void => Vec::new(),
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
                    self.tuple_result_slots(payload(), declaration.map(Type::ok_type).as_ref())
                } else {
                    vec![payload().clone()]
                }
            }
            Some(LoweredReturnAbi::Tuple { .. }) => self.tuple_result_slots(result, declaration),
        };
        FunctionLayout {
            parameters,
            result_type: result.logical_type().clone(),
            results,
            return_abi,
        }
    }

    fn tuple_result_slots(
        &self,
        tuple: &ValueLayout,
        declaration: Option<&Type>,
    ) -> Vec<ValueLayout> {
        let declared = declaration
            .map(|ty| tuple_element_types(&self.facts.peel_alias(ty)))
            .unwrap_or_default();
        tuple
            .tuple_elements()
            .expect("a tuple-shaped return ABI has a tuple layout")
            .iter()
            .enumerate()
            .map(|(index, element)| match element {
                ValueLayout::TaggedOption {
                    option_type,
                    payload,
                } if self
                    .facts
                    .is_nullable_option(declared.get(index).unwrap_or(option_type)) =>
                {
                    ValueLayout::NullableOption {
                        option_type: option_type.clone(),
                        payload: payload.clone(),
                    }
                }
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
        let alias_declaration = self.facts.function_alias_declaration(ty);
        let resolved_declaration = alias_declaration
            .clone()
            .or_else(|| declaration.map(|ty| self.facts.peel_alias(ty)));
        let alias_return = alias_declaration.as_ref().and_then(Type::get_function_ret);
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
            let underlying = if alias_declaration.is_none()
                && let Some(declared) = self.facts.alias_declaration(ty)
            {
                self.value_layout_with_declaration(&resolved, origin, declared)
            } else {
                self.value_layout_resolved(
                    resolved,
                    origin,
                    resolved_declaration.as_ref(),
                    alias_return,
                )
            };
            return ValueLayout::Named {
                named_type: ty.clone(),
                underlying: Box::new(underlying),
            };
        }
        self.value_layout_resolved(
            resolved,
            origin,
            resolved_declaration.as_ref(),
            alias_return,
        )
    }

    fn value_layout_resolved(
        &self,
        ty: Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
        physical_return: Option<&Type>,
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
                let declared_error = declaration
                    .filter(|ty| is_prelude_container_type(ty))
                    .map(Type::err_type);
                let error = Box::new(self.value_layout_with_hint(
                    &ty.err_type(),
                    origin.nested(),
                    declared_error.as_ref(),
                ));
                return ValueLayout::Fallible {
                    fallible_type: ty,
                    payload,
                    error,
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
                    let component_layout = |ty: &Type, declaration: Option<&Type>| {
                        if physical_return.is_some()
                            && let Some(declaration) = declaration
                        {
                            self.value_layout_with_declaration(ty, origin.nested(), declaration)
                        } else {
                            self.value_layout_with_hint(ty, origin.nested(), declaration)
                        }
                    };
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
                            component_layout(
                                &parameter.ty,
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
                    let return_abi =
                        self.slot_return_abi(physical_return.unwrap_or(&result_type), origin);
                    let result = self.callable_result_layout(
                        &result_type,
                        origin.nested(),
                        physical_return.and(declared_result),
                        &return_abi,
                    );
                    return ValueLayout::Function {
                        function_type: ty,
                        layout: self.function_layout(
                            parameters,
                            &result,
                            return_abi,
                            declared_result,
                        ),
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
