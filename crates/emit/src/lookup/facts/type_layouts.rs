use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::return_abi_rules::slot_return_abi;
use crate::lookup::facts::value_layout::{
    FallibleKind, FunctionLayout, PayloadSlots, ResultProducer, ReturnLayout, ValueLayout,
};
use crate::target::ir::prelude::is_prelude_container_type;
use crate::target::types::go_type::GoTypeKind;
use crate::target::types::return_abi::{
    CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout,
};
use syntax::program::{
    FieldSelection, GoOptionRepresentation, SlotOrigin, go_option_representation, is_go_import,
};
use syntax::types::{CompoundKind, FunctionType, Symbol, Type};

impl EmitFacts<'_> {
    pub(crate) fn lowered_tuple_slot_layouts(&self, tuple_ty: &Type) -> Vec<ValueLayout> {
        let layout = self.value_layout(tuple_ty, SlotOrigin::Lisette);
        let elements = layout
            .tuple_elements()
            .expect("a tuple type has a tuple layout");
        self.tuple_result_slots(elements, Some(tuple_ty))
    }

    pub(crate) fn prelude_receiver_method_layouts(
        &self,
        receiver_ty: &Type,
        declaration: &FunctionType,
        instantiated: &FunctionType,
        mut target: FunctionLayout,
    ) -> Option<(FunctionLayout, FunctionLayout)> {
        let (receiver_param, parameters) = declaration.params.split_first()?;
        let receiver_declaration = receiver_param.ty.strip_refs();
        let type_params = receiver_declaration.get_type_params()?;
        let receiver_layout = self.dereferenced_layout(receiver_ty);
        let bindings = receiver_slot_bindings(type_params, &receiver_layout)?;
        let canonical_receiver = self.receiver_layout(receiver_ty);
        let canonical_bindings = receiver_slot_bindings(type_params, &canonical_receiver)
            .expect("prelude receiver has the same container after alias expansion");
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
            target.return_abi(),
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
        let declared = self.peel_alias(declaration);
        let actual = self.peel_alias(ty);
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
                *layout = self.function_layout_with_receiver_slots(actual, declared, bindings);
            }
            ValueLayout::Plain(_)
            | ValueLayout::Fallible { .. }
            | ValueLayout::TaggedOption { .. }
            | ValueLayout::NullableOption { .. }
            | ValueLayout::PointerOption { .. }
            | ValueLayout::Reference { .. }
            | ValueLayout::Slice { .. }
            | ValueLayout::Map { .. }
            | ValueLayout::Array { .. }
            | ValueLayout::Tuple { .. }
            | ValueLayout::Named { .. } => {}
        }
        layout
    }

    fn function_layout_with_receiver_slots(
        &self,
        actual: &FunctionType,
        declared: &FunctionType,
        bindings: &[(&Type, &ValueLayout)],
    ) -> FunctionLayout {
        let parameters = actual
            .params
            .iter()
            .zip(&declared.params)
            .map(|(actual, declared)| {
                self.layout_with_receiver_slots(&actual.ty, &declared.ty, bindings)
            })
            .collect();

        let result =
            self.layout_with_receiver_slots(&actual.return_type, &declared.return_type, bindings);
        self.function_layout(
            parameters,
            &result,
            slot_return_abi(self, &declared.return_type, SlotOrigin::Lisette),
            Some(&declared.return_type),
        )
    }

    pub(crate) fn field_slot_layout(
        &self,
        owner_type: &Type,
        selection: Option<&FieldSelection>,
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
        if let Some(package) = owner_type.as_import_namespace()
            && let Some(layout) = self.go_variable_layout(package, field, value_type)
        {
            return Some(layout);
        }
        let selection = selection?;
        if is_go_import(&selection.owner) {
            let slot = self
                .go_field(selection.owner.as_str(), field)
                .unwrap_or_else(|| {
                    panic!(
                        "internal error: `{}.{field}` has no Go slot",
                        selection.owner
                    )
                });
            return Some(self.value_layout_with_declaration(
                value_type,
                slot.origin,
                &slot.declared_type,
            ));
        }
        let function_type = self.resolve_to_function_type(value_type);
        let declaration = self
            .function_alias_declaration(value_type)
            .unwrap_or_else(|| selection.declared_ty.clone());
        let layout = self.value_layout_with_declaration(
            function_type.as_ref().unwrap_or(value_type),
            self.function_type_origin(value_type, SlotOrigin::Lisette),
            &declaration,
        );
        Some(layout)
    }

    fn go_variable_layout(
        &self,
        package: &str,
        member: &str,
        value_type: &Type,
    ) -> Option<ValueLayout> {
        let slot = self.go_variable(Symbol::from_parts(package, member).as_str())?;
        Some(self.value_layout_with_declaration(value_type, slot.origin, &slot.declared_type))
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
        let resolved = abi.lowered().map(|_| self.peel_alias(ty));
        let declared = abi
            .lowered()
            .and_then(|_| declaration.map(|ty| self.peel_alias(ty)));
        let result = resolved.as_ref().unwrap_or(ty);
        let layout = declared.as_ref().or(declaration).map_or_else(
            || self.value_layout(result, origin),
            |declaration| self.value_layout_with_declaration(result, origin, declaration),
        );
        if result != ty {
            ValueLayout::Named {
                named_type: ty.clone(),
                underlying: Box::new(layout),
                go_alias: self.is_transparent_alias(ty),
            }
        } else {
            layout
        }
    }

    pub(crate) fn dereferenced_layout(&self, ty: &Type) -> ValueLayout {
        self.value_layout(&ty.strip_refs(), SlotOrigin::Lisette)
    }

    pub(crate) fn receiver_layout(&self, ty: &Type) -> ValueLayout {
        self.value_layout(&self.receiver_shape(ty), SlotOrigin::Lisette)
    }

    pub(crate) fn interface_result_layout(
        &self,
        go_method: Option<&str>,
        return_type: &Type,
    ) -> ValueLayout {
        match go_method.and_then(|id| self.go_callable_return_slot(id)) {
            Some(slot) => {
                self.target_layout_with_declaration(return_type, slot.origin, &slot.declared_type)
            }
            None => self.value_layout(&self.peel_alias(return_type), SlotOrigin::Lisette),
        }
    }

    pub(crate) fn target_layout_with_declaration(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: &Type,
    ) -> ValueLayout {
        self.value_layout_with_declaration(&self.peel_alias(ty), origin, declaration)
    }

    pub(crate) fn value_layout_with_declaration(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: &Type,
    ) -> ValueLayout {
        if matches!(ty, Type::Function(_))
            && let Some(declaration) = self.resolve_to_function_type(declaration)
        {
            return self.value_layout_resolved(
                ty.clone(),
                origin,
                Some(&declaration),
                declaration
                    .as_function_type()
                    .map(|function| function.return_type.as_ref()),
            );
        }
        if self.resolve_to_function_type(declaration).is_none()
            && let Some(declaration) = self.alias_declaration(declaration)
        {
            return self.value_layout_with_declaration(ty, origin, declaration);
        }
        let mut layout = self.value_layout_with_hint(ty, origin, Some(declaration));
        self.refine_with_declaration(&mut layout, origin, &self.peel_alias(declaration));
        layout
    }

    fn refine_with_declaration(
        &self,
        layout: &mut ValueLayout,
        origin: SlotOrigin,
        declaration: &Type,
    ) {
        match layout {
            ValueLayout::TaggedOption {
                option_type: ty,
                payload,
            }
            | ValueLayout::NullableOption {
                option_type: ty,
                payload,
            }
            | ValueLayout::PointerOption {
                option_type: ty,
                payload,
            } if is_prelude_container_type(declaration) => {
                **payload = self.value_layout_with_declaration(
                    &ty.ok_type(),
                    origin.nested(),
                    &declaration.ok_type(),
                );
            }
            ValueLayout::Fallible {
                fallible_type: ty,
                payload,
                error,
                ..
            } if is_prelude_container_type(declaration) => {
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
                    if let Some(declared) = tuple_hint(Some(declaration), index) {
                        *element = self.value_layout_with_declaration(
                            element.logical_type(),
                            origin.nested(),
                            declared,
                        );
                    }
                }
            }
            ValueLayout::Plain(_)
            | ValueLayout::Fallible { .. }
            | ValueLayout::TaggedOption { .. }
            | ValueLayout::NullableOption { .. }
            | ValueLayout::PointerOption { .. }
            | ValueLayout::Reference { .. }
            | ValueLayout::Slice { .. }
            | ValueLayout::Map { .. }
            | ValueLayout::Array { .. }
            | ValueLayout::Function { .. }
            | ValueLayout::Named { .. } => {}
        }
    }

    pub(crate) fn function_layout(
        &self,
        parameters: Vec<ValueLayout>,
        result: &ValueLayout,
        return_abi: CallableReturnAbi,
        declaration: Option<&Type>,
    ) -> FunctionLayout {
        let payload_slots = |encoding: PayloadLayout| {
            let payload = result.payload()?;
            Some(match encoding {
                PayloadLayout::Packed => PayloadSlots::Packed(payload.clone()),
                PayloadLayout::Flattened => PayloadSlots::Flattened(self.tuple_result_slots(
                    payload.tuple_elements()?,
                    declaration.map(Type::ok_type).as_ref(),
                )),
            })
        };
        let returns = match &return_abi {
            CallableReturnAbi::Void => Some(ReturnLayout::Void),
            CallableReturnAbi::Value => Some(ReturnLayout::Value(result.clone())),
            CallableReturnAbi::Lowered(LoweredReturnAbi::BareError) => {
                Some(ReturnLayout::BareError)
            }
            CallableReturnAbi::Lowered(LoweredReturnAbi::Result { payload }) => {
                payload_slots(*payload).map(ReturnLayout::Result)
            }
            CallableReturnAbi::Lowered(LoweredReturnAbi::Partial { payload }) => {
                payload_slots(*payload).map(ReturnLayout::Partial)
            }
            CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
                payload,
            })) => payload_slots(*payload).map(ReturnLayout::CommaOk),
            CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                result.payload().cloned().map(ReturnLayout::Nullable)
            }
            CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(
                value,
            ))) => result.payload().map(|payload| ReturnLayout::Sentinel {
                value: *value,
                payload: payload.clone(),
            }),
            CallableReturnAbi::Lowered(LoweredReturnAbi::Tuple { arity }) => {
                result.tuple_elements().map(|elements| ReturnLayout::Tuple {
                    arity: *arity,
                    slots: self.tuple_result_slots(elements, declaration),
                })
            }
        }
        .unwrap_or_else(|| {
            unreachable!("the return ABI {return_abi:?} does not fit the result layout {result:?}")
        });

        FunctionLayout {
            parameters,
            result_layout: Box::new(result.clone()),
            returns: Box::new(returns),
            producer: ResultProducer::Go,
        }
    }

    pub(crate) fn is_nullable_option(&self, ty: &Type) -> bool {
        let ty = self.peel_alias(ty);
        ty.is_option() && self.is_nilable_go_type(&ty.ok_type())
    }

    pub(crate) fn target_value_layout(&self, ty: &Type) -> ValueLayout {
        self.value_layout(&self.peel_alias(ty), SlotOrigin::Lisette)
    }

    pub(crate) fn return_value_layout(
        &self,
        abi: &CallableReturnAbi,
        return_ty: &Type,
    ) -> ValueLayout {
        if abi.is_lowered() {
            self.target_value_layout(return_ty)
        } else {
            self.value_layout(return_ty, SlotOrigin::Lisette)
        }
    }

    pub(crate) fn function_type_origin(&self, fn_ty: &Type, origin: SlotOrigin) -> SlotOrigin {
        if origin.declared_by_go() {
            origin
        } else if self.is_go_named_function_type(fn_ty) {
            SlotOrigin::GoParameter
        } else if let Some(target) = self.alias_declaration(fn_ty) {
            self.function_type_origin(target, origin)
        } else {
            origin
        }
    }

    pub(crate) fn physical_go_kind(&self, ty: &Type) -> GoTypeKind {
        match self.peel_underlying(ty) {
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
                .is_some_and(|inner| matches!(self.peel_underlying(inner), Type::Array { .. })) =>
            {
                GoTypeKind::PointerToArray
            }
            Type::Array { .. } => GoTypeKind::Array,
            Type::Simple(_)
            | Type::Compound { .. }
            | Type::Nominal { .. }
            | Type::ImportNamespace(_)
            | Type::Function(_)
            | Type::Var { .. }
            | Type::Uninferred
            | Type::Ignored
            | Type::Forall { .. }
            | Type::Parameter(_)
            | Type::Never
            | Type::Tuple(_)
            | Type::Error
            | Type::ReceiverPlaceholder => GoTypeKind::Value,
        }
    }

    fn tuple_result_slots(
        &self,
        elements: &[ValueLayout],
        declaration: Option<&Type>,
    ) -> Vec<ValueLayout> {
        let declared = declaration.map(|ty| self.tuple_or_type_arguments(ty));
        elements
            .iter()
            .enumerate()
            .map(|(index, element)| match element {
                ValueLayout::TaggedOption {
                    option_type,
                    payload,
                } if self.is_nullable_option(
                    declared
                        .as_ref()
                        .map_or(option_type, |declared| &declared[index]),
                ) =>
                {
                    ValueLayout::NullableOption {
                        option_type: option_type.clone(),
                        payload: payload.clone(),
                    }
                }
                element @ ValueLayout::Plain(_)
                | element @ ValueLayout::Fallible { .. }
                | element @ ValueLayout::TaggedOption { .. }
                | element @ ValueLayout::NullableOption { .. }
                | element @ ValueLayout::PointerOption { .. }
                | element @ ValueLayout::Reference { .. }
                | element @ ValueLayout::Slice { .. }
                | element @ ValueLayout::Map { .. }
                | element @ ValueLayout::Array { .. }
                | element @ ValueLayout::Function { .. }
                | element @ ValueLayout::Tuple { .. }
                | element @ ValueLayout::Named { .. } => element.clone(),
            })
            .collect()
    }

    fn value_layout_with_hint(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> ValueLayout {
        if let Some(layout) = self.go_alias_layout(ty, origin, declaration) {
            return layout;
        }
        let alias_declaration = self.function_alias_declaration(ty);
        let resolved_declaration = alias_declaration
            .clone()
            .or_else(|| declaration.map(|ty| self.peel_alias(ty)));
        let alias_return = alias_declaration
            .as_ref()
            .and_then(Type::as_function_type)
            .map(|function| function.return_type.as_ref());
        if resolved_declaration
            .as_ref()
            .is_some_and(|ty| self.declaration_is_open(ty))
        {
            return self.value_layout_with_hint(ty, SlotOrigin::Lisette, None);
        }

        let origin = self.function_type_origin(ty, origin);
        let resolved = self.peel_alias(ty);
        if resolved != *ty {
            let value_declaration = if alias_declaration.is_none() {
                self.alias_declaration(ty)
            } else {
                None
            };
            let build = |origin| match value_declaration {
                Some(declared) => self.value_layout_with_declaration(&resolved, origin, declared),
                None => self.value_layout_resolved(
                    resolved.clone(),
                    origin,
                    resolved_declaration.as_ref(),
                    alias_return,
                ),
            };

            let underlying = build(origin);
            let declared_origin = self.function_type_origin(ty, SlotOrigin::Lisette);
            if origin != declared_origin
                && !self.is_go_named_function_type(ty)
                && !underlying.same_representation(&build(declared_origin))
            {
                return underlying;
            }
            return ValueLayout::Named {
                named_type: ty.clone(),
                underlying: Box::new(underlying),
                go_alias: self.is_transparent_alias(ty),
            };
        }
        self.value_layout_resolved(
            resolved,
            origin,
            resolved_declaration.as_ref(),
            alias_return,
        )
    }

    fn go_alias_layout(
        &self,
        ty: &Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> Option<ValueLayout> {
        let go_target = self.peel_go_alias(ty);
        if go_target == *ty.unwrap_forall() || go_target == self.peel_alias(ty) {
            return None;
        }
        Some(ValueLayout::Named {
            named_type: ty.clone(),
            underlying: Box::new(self.value_layout_with_hint(&go_target, origin, declaration)),
            go_alias: true,
        })
    }

    fn declaration_is_open(&self, declaration: &Type) -> bool {
        self.resolves_to_unknown(declaration)
            || matches!(
                declaration.unwrap_forall(),
                Type::Parameter(_) | Type::Var { .. } | Type::ReceiverPlaceholder
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
            return self.container_value_layout(ty, origin, declaration);
        }

        let nested =
            |ty: &Type, hint: Option<&Type>| self.value_layout_with_hint(ty, origin.nested(), hint);
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
                    pointee: Box::new(nested(
                        pointee,
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
                    element: Box::new(nested(element, compound_hint(declaration, *kind, 0))),
                }
            }
            Type::Compound {
                kind: CompoundKind::Map,
                args,
                ..
            } => match args.as_slice() {
                [key, value] => ValueLayout::Map {
                    collection_type: ty.clone(),
                    key: Box::new(nested(
                        key,
                        compound_hint(declaration, CompoundKind::Map, 0),
                    )),
                    value: Box::new(nested(
                        value,
                        compound_hint(declaration, CompoundKind::Map, 1),
                    )),
                },
                _ => ValueLayout::Plain(ty),
            },
            Type::Array { length, element } => ValueLayout::Array {
                array_type: ty.clone(),
                length: *length,
                element: Box::new(nested(element, array_hint(declaration))),
            },
            Type::Tuple(elements) => ValueLayout::Tuple {
                tuple_type: ty.clone(),
                elements: elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| nested(element, tuple_hint(declaration, index)))
                    .collect(),
            },
            Type::Error => ValueLayout::Plain(ty),
            Type::Uninferred | Type::ReceiverPlaceholder => {
                panic!("internal error: {ty:?} has no value layout")
            }
            Type::Simple(_)
            | Type::Compound { .. }
            | Type::Nominal { .. }
            | Type::ImportNamespace(_)
            | Type::Function(_)
            | Type::Var { .. }
            | Type::Ignored
            | Type::Forall { .. }
            | Type::Parameter(_)
            | Type::Never => self.nominal_value_layout(ty, origin, declaration, physical_return),
        }
    }

    fn nominal_value_layout(
        &self,
        ty: Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
        physical_return: Option<&Type>,
    ) -> ValueLayout {
        if let Some(Type::Function(function_type)) = self.resolve_to_function_type(&ty) {
            return self.function_value_layout(
                ty,
                &function_type,
                origin,
                declaration,
                physical_return,
            );
        }
        if let Some(underlying) = self.newtype_underlying(&ty) {
            let declared_underlying = declaration.and_then(|ty| self.newtype_underlying(ty));
            return ValueLayout::Named {
                named_type: ty,
                underlying: Box::new(self.value_layout_with_hint(
                    &underlying,
                    origin.nested(),
                    declared_underlying.as_ref(),
                )),
                go_alias: false,
            };
        }
        ValueLayout::Plain(ty)
    }

    fn container_value_layout(
        &self,
        ty: Type,
        origin: SlotOrigin,
        declaration: Option<&Type>,
    ) -> ValueLayout {
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
            let kind = if ty.is_partial() {
                FallibleKind::Partial
            } else {
                FallibleKind::Result
            };
            return ValueLayout::Fallible {
                fallible_type: ty,
                kind,
                payload,
                error,
            };
        }

        match go_option_representation(&ty, origin, |id| Some(self.definition(id))) {
            GoOptionRepresentation::Tagged => ValueLayout::TaggedOption {
                option_type: ty,
                payload,
            },
            GoOptionRepresentation::Nullable => ValueLayout::NullableOption {
                option_type: ty,
                payload,
            },
            GoOptionRepresentation::Pointer => ValueLayout::PointerOption {
                option_type: ty,
                payload,
            },
        }
    }

    fn function_value_layout(
        &self,
        ty: Type,
        function_type: &FunctionType,
        origin: SlotOrigin,
        declaration: Option<&Type>,
        physical_return: Option<&Type>,
    ) -> ValueLayout {
        let component_layout = |ty: &Type, declaration: Option<&Type>| {
            if physical_return.is_some()
                && let Some(declaration) = declaration
            {
                self.value_layout_with_declaration(ty, origin.nested(), declaration)
            } else {
                self.value_layout_with_hint(ty, origin.nested(), declaration)
            }
        };

        let declared_function = declaration.and_then(|ty| self.resolve_to_function_type(ty));
        let declared_function = declared_function.as_ref().and_then(Type::as_function_type);
        let declared_parameters = declared_function.map_or(&[][..], |function| &function.params);
        let parameters = function_type
            .params
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                component_layout(
                    &parameter.ty,
                    declared_parameters.get(index).map(|param| &param.ty),
                )
            })
            .collect();

        let result_type = function_type.return_type.as_ref();
        let declared_result = declared_function.map(|function| function.return_type.as_ref());
        let return_abi = slot_return_abi(self, physical_return.unwrap_or(result_type), origin);
        let result = self.callable_result_layout(
            result_type,
            origin.nested(),
            physical_return.and(declared_result),
            &return_abi,
        );

        ValueLayout::Function {
            function_type: ty,
            layout: self.function_layout(parameters, &result, return_abi, declared_result),
        }
    }
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

fn receiver_slot_bindings<'a>(
    type_params: &'a [Type],
    receiver: &'a ValueLayout,
) -> Option<Vec<(&'a Type, &'a ValueLayout)>> {
    let slots = match receiver.underlying() {
        ValueLayout::TaggedOption { payload, .. } => vec![payload.as_ref()],
        ValueLayout::Fallible { payload, error, .. } => vec![payload.as_ref(), error.as_ref()],
        ValueLayout::Plain(_)
        | ValueLayout::NullableOption { .. }
        | ValueLayout::PointerOption { .. }
        | ValueLayout::Reference { .. }
        | ValueLayout::Slice { .. }
        | ValueLayout::Map { .. }
        | ValueLayout::Array { .. }
        | ValueLayout::Function { .. }
        | ValueLayout::Tuple { .. }
        | ValueLayout::Named { .. } => return None,
    };
    Some(type_params.iter().zip(slots).collect())
}
