use super::aliasing::{PlaceKey, render_place};
use crate::checker::EnvResolve;
use rustc_hash::FxHashSet;
use syntax::ast::{CallTypeArguments, Expression, IdentifierResolution, StructFields};
use syntax::program::{
    CallOperation, Definition, DefinitionBody, DotAccessResolution, NativeTypeKind, ValueKind,
};
use syntax::program::{resolved_definition, resolved_instantiation};
use syntax::types::{
    FunctionParameter, GO_IMPORT_PREFIX, RangeKind, SubstitutionMap, Symbol, Type,
    peel_to_range_type,
};

use crate::checker::infer::InferCtx;
use crate::checker::infer::value_uses::is_instance_method_access;
use syntax::program::AliasKind;

/// What a call expression resolved to during type checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallKind {
    /// Regular function or method call
    Regular,
    /// Tuple struct constructor (e.g., `Point(1, 2)`)
    TupleStructConstructor,
    /// Type assertion (`assert_type`)
    AssertType,
    /// Zero value of a type (`zero`)
    Zero,
    /// UFCS method call: `receiver.method()` where method is a free function
    UfcsMethod,
    /// Native type constructor (e.g., `Channel.new`, `Map.new`, `Slice.new`)
    NativeConstructor(NativeTypeKind),
    /// Native type instance method via dot access (e.g., `slice.append(x)`)
    NativeMethod(NativeTypeKind),
    /// Native type method via identifier (e.g., `Slice.contains(s, x)`)
    NativeMethodIdentifier(NativeTypeKind),
    /// Receiver method in UFCS syntax: `Type.method(receiver, args)`
    ReceiverMethodUfcs { is_public: bool },
}

impl InferCtx<'_> {
    pub(super) fn writable_receiver_place(&self, callee: &Expression) -> Option<PlaceKey> {
        let Expression::DotAccess {
            expression: receiver,
            member,
            resolution: DotAccessResolution::InstanceMethod { .. },
            ..
        } = callee.unwrap_parens()
        else {
            return None;
        };
        let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();
        let store = self.store;
        let method = self.method_of_type(store, &receiver_ty, member)?;
        let Type::Function(function) = method.method.ty.unwrap_forall() else {
            return None;
        };
        if !self
            .store
            .parameter_grants_write(&function.params.first()?.ty)
        {
            return None;
        }
        super::aliasing::place_key(receiver.unwrap_parens())
    }

    pub(super) fn classify_call(&self, callee: &Expression) -> CallKind {
        let store = self.store;
        let callee = callee.unwrap_parens();
        match callee {
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } => {
                let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();
                let peeled = store.receiver_shape(&receiver_ty);

                let is_ufcs_member = |ty: &Type| {
                    matches!(ty, Type::Nominal { id, .. }
                        if store.is_ufcs_method(id, member))
                };
                if is_ufcs_member(&receiver_ty) || is_ufcs_member(&peeled) {
                    return CallKind::UfcsMethod;
                }

                // Native method: receiver.method() on Slice/Map/Channel/etc.
                if let Some(kind) = NativeTypeKind::from_type(&peeled) {
                    return CallKind::NativeMethod(kind);
                }

                // Cross-package tuple struct constructor (e.g. `mod.Point(1, 2)`)
                if let Some(package_id) = receiver
                    .get_type()
                    .resolve_in(&self.env)
                    .as_import_namespace()
                {
                    let qualified = Symbol::from_parts(package_id, member);
                    if matches!(
                        store.get_definition(&qualified).map(|d| &d.body),
                        Some(DefinitionBody::Struct {
                            fields: StructFields::Tuple(_),
                            ..
                        })
                    ) {
                        return CallKind::TupleStructConstructor;
                    }
                }
            }
            Expression::Identifier {
                value, resolution, ..
            } => {
                let qualified = self.qualify_name(value);
                let definition = store.get_definition(&qualified);
                let shadowed = matches!(resolution, IdentifierResolution::Binding(_));
                if definition.is_none() && !shadowed {
                    match value.as_str() {
                        "assert_type" => return CallKind::AssertType,
                        "zero" => return CallKind::Zero,
                        _ => {}
                    }
                }
                if self.is_tuple_struct_definition(definition, callee) {
                    return CallKind::TupleStructConstructor;
                }

                if let Some(kind) = NativeTypeKind::from_constructor_path(value) {
                    return CallKind::NativeConstructor(kind);
                }

                // Native method identifier: Slice.contains(s, x), Map.delete(m, k), etc.
                if let Some((prefix, _method)) = value.split_once('.')
                    && let Some(kind) = NativeTypeKind::from_name(prefix)
                {
                    return CallKind::NativeMethodIdentifier(kind);
                }

                // Receiver method UFCS: Type.method(receiver, args)
                if let Some(kind) = self.try_classify_receiver_ufcs(value) {
                    return kind;
                }
            }
            _ => {}
        }
        CallKind::Regular
    }

    fn tuple_struct_constructor(&self, callee: &Expression) -> Option<CallOperation> {
        let callee_ty = callee.get_type().resolve_in(&self.env);
        let return_ty = callee_ty.as_function_type()?.return_type.as_ref().clone();
        let Type::Nominal { id, .. } = self.store.peel_alias(&return_ty) else {
            return None;
        };
        let is_newtype = self
            .store
            .get_definition(&id)
            .is_some_and(Definition::is_newtype);
        Some(CallOperation::TupleStructConstructor {
            owner: id,
            is_newtype,
        })
    }

    /// The operation that a call with this callee and classification runs, or None when the instance method access that the call names selected it.
    pub(super) fn select_call_operation(
        &self,
        callee: &Expression,
        call_kind: CallKind,
    ) -> Option<CallOperation> {
        let callee = callee.unwrap_value_wrappers();
        let native_method = || match callee {
            Expression::DotAccess { member, .. } => member.clone(),
            Expression::Identifier { value, .. } => value
                .split_once('.')
                .map_or_else(|| value.clone(), |(_, method)| method.into()),
            _ => Default::default(),
        };
        let local = matches!(
            callee,
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(_),
                ..
            }
        );
        let declared_by_go = resolved_definition(callee)
            .is_some_and(|definition| definition.starts_with(GO_IMPORT_PREFIX));
        let selected_by_access =
            || (!is_instance_method_access(callee)).then_some(CallOperation::Function);
        let operation = match call_kind {
            _ if local => CallOperation::Function,
            CallKind::TupleStructConstructor => self.tuple_struct_constructor(callee)?,
            _ if declared_by_go => return selected_by_access(),
            CallKind::AssertType => CallOperation::AssertType,
            CallKind::Zero => CallOperation::Zero,
            CallKind::NativeConstructor(kind) => CallOperation::NativeConstructor {
                kind,
                method: native_method(),
            },
            CallKind::NativeMethod(kind) | CallKind::NativeMethodIdentifier(kind) => {
                CallOperation::Native {
                    kind,
                    method: native_method(),
                }
            }
            CallKind::UfcsMethod => CallOperation::UfcsFunction,
            CallKind::ReceiverMethodUfcs { is_public } => {
                CallOperation::ReceiverMethod { is_public }
            }
            CallKind::Regular => return selected_by_access(),
        };
        Some(operation)
    }

    /// The instantiation of `callee`, with the explicit type arguments of its call.
    pub(super) fn call_type_arguments(
        &self,
        callee: &Expression,
        explicit: &CallTypeArguments,
        declared: &Type,
    ) -> SubstitutionMap {
        let mut type_arguments = resolved_instantiation(callee.unwrap_value_wrappers())
            .cloned()
            .unwrap_or_default();
        if let Type::Forall { vars, .. } = declared
            && let Some(explicit) = explicit.resolved_types()
            && let Some(offset) = vars.len().checked_sub(explicit.len())
        {
            for (name, argument) in vars[offset..].iter().zip(explicit.iter()) {
                type_arguments.insert(name.clone(), argument.clone());
            }
        }
        type_arguments
    }

    /// Classify `Type.method(receiver, args)` as `ReceiverMethodUfcs`.
    /// Uses scope-aware name resolution instead of the old suffix-matching heuristic.
    pub(super) fn try_classify_receiver_ufcs(&self, value: &str) -> Option<CallKind> {
        let store = self.store;
        let last_dot = value.rfind('.')?;
        let method = &value[last_dot + 1..];
        let type_part = &value[..last_dot];

        let qualified_name = self.lookup_qualified_name(store, type_part)?;

        // Follow type-alias chains through Simple/Compound underlying types
        // (e.g. `type MyString = string` → look up methods on `prelude.string`).
        let method_ty = store
            .get_definition(&qualified_name)
            .and_then(|definition| match &definition.body {
                DefinitionBody::Struct { methods, .. } => methods.get(method).cloned(),
                DefinitionBody::Enum { methods, .. } => methods.get(method).cloned(),
                DefinitionBody::TypeAlias { alias, methods, .. } => {
                    methods.get(method).cloned().or_else(|| {
                        // Follow the alias to its underlying type.
                        let underlying = match alias {
                            AliasKind::Transparent { target, .. } => target,
                            AliasKind::Opaque(_) => return None,
                        };
                        let underlying_key: Option<String> = match underlying {
                            Type::Simple(kind) => Some(format!("prelude.{}", kind.leaf_name())),
                            Type::Compound { kind, .. } => {
                                Some(format!("prelude.{}", kind.leaf_name()))
                            }
                            _ => None,
                        };
                        underlying_key.and_then(|k| store.get_own_methods(&k)?.get(method).cloned())
                    })
                }
                _ => None,
            })?;

        let has_self = match &method_ty.ty {
            Type::Function(f) => !f.params.is_empty(),
            Type::Forall { body, .. } => {
                if let Type::Function(f) = body.as_ref() {
                    !f.params.is_empty()
                } else {
                    false
                }
            }
            _ => false,
        };

        if !has_self {
            return None;
        }

        // If it's a UFCS-lowered method, skip: the emitter handles it differently
        if store.is_ufcs_method(&qualified_name, method) {
            return None;
        }

        let is_public = store
            .get_method(&qualified_name, method)
            .map(|method| method.visibility.is_public())
            .unwrap_or(false);

        Some(CallKind::ReceiverMethodUfcs { is_public })
    }

    /// Check if a definition (or type alias target) is a multi-field tuple struct constructor.
    pub(super) fn is_tuple_struct_definition(
        &self,
        definition: Option<&Definition>,
        callee: &Expression,
    ) -> bool {
        let store = self.store;
        // Direct tuple struct
        if matches!(
            definition.map(|d| &d.body),
            Some(DefinitionBody::Struct {
                fields: StructFields::Tuple(_),
                ..
            })
        ) {
            return true;
        }
        // Type alias → follow to the underlying struct via the callee's return type
        if matches!(
            definition.map(|d| &d.body),
            Some(DefinitionBody::TypeAlias { .. })
        ) {
            let ty = callee.get_type().resolve_in(&self.env);
            let return_ty = match ty.unwrap_forall() {
                Type::Function(f) => f.return_type.as_ref().clone(),
                _ => return false,
            };
            if let Type::Nominal { id, .. } = return_ty.resolve_in(&self.env) {
                return matches!(
                    store.get_definition(&id).map(|d| &d.body),
                    Some(DefinitionBody::Struct {
                        fields: StructFields::Tuple(_),
                        ..
                    })
                );
            }
        }
        false
    }

    pub(super) fn is_panic_call(&self, expression: &Expression) -> bool {
        resolved_definition(expression) == Some("prelude.panic")
    }

    pub(super) fn mark_native_mutating_call(&mut self, callee: &Expression) {
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee
        else {
            return;
        };
        let receiver_ty = receiver.get_type().resolve_in(&self.env);

        let is_mutating = NativeTypeKind::from_type(&self.store.receiver_shape(&receiver_ty))
            .is_some_and(|kind| {
                syntax::program::NativeOperation::new(kind, member).mutates_receiver()
            });
        if !is_mutating {
            return;
        }
        if let Some(var_name) = receiver.get_var_name()
            && let Some(binding_id) = self.scopes.lookup_binding_id(&var_name)
        {
            self.facts.mark_alias_mutated(binding_id);
        }
    }

    pub(super) fn check_aliased_writable_arguments(
        &mut self,
        arguments: &[Expression],
        parameters: &[FunctionParameter],
        variadic: Option<&FunctionParameter>,
        writable_receiver: Option<PlaceKey>,
    ) {
        let places: Vec<Option<PlaceKey>> = arguments
            .iter()
            .map(|argument| {
                let argument = argument.unwrap_parens();
                if self.names_no_storage(argument) {
                    None
                } else {
                    super::aliasing::place_key(argument)
                }
            })
            .collect();

        let parameter_at = |i: usize| {
            parameters.get(i).or(if i >= parameters.len() {
                variadic
            } else {
                None
            })
        };

        let writable_positions: Vec<bool> = (0..arguments.len())
            .map(|i| {
                parameter_at(i).is_some_and(|param| {
                    self.store
                        .parameter_grants_write(&param.ty.resolve_in(&self.env))
                })
            })
            .collect();

        let mutating_positions: Vec<bool> = (0..arguments.len())
            .map(|i| {
                writable_positions[i]
                    || parameter_at(i).is_some_and(|param| {
                        let param_ty = param.ty.resolve_in(&self.env);
                        self.store.is_interface(&param_ty)
                            && self.interface_writes_through_receiver(
                                &arguments[i].get_type().resolve_in(&self.env),
                                &param_ty,
                            )
                    })
            })
            .collect();

        let mut reported_places: Vec<&PlaceKey> = Vec::new();
        for (i, argument) in arguments.iter().enumerate() {
            let writable_position = writable_positions[i];
            let Some(place) = places[i].as_ref() else {
                continue;
            };
            if mutating_positions[i]
                && let Some((binding_id, _)) = self.binding_for_place(argument)
            {
                // The callee gets a copy of a named argument and cannot rebind the name.
                if matches!(argument.unwrap_parens(), Expression::Identifier { .. }) {
                    if self.peel_resolved(&argument.get_type()).is_ref() {
                        self.facts.mark_written_through(binding_id);
                    } else {
                        self.facts.mark_mutated(binding_id);
                    }
                } else {
                    self.facts.mark_alias_mutated(binding_id);
                }
            }

            let aliases_receiver = writable_receiver
                .as_ref()
                .is_some_and(|other| other.overlaps(place))
                && self.argument_shares_storage(
                    &argument.get_type().resolve_in(&self.env),
                    &mut Default::default(),
                );
            let aliases_argument = writable_position
                && places.iter().enumerate().any(|(j, other)| {
                    j != i
                        && other.as_ref().is_some_and(|other| other.overlaps(place))
                        && self.argument_shares_storage(
                            &arguments[j].get_type().resolve_in(&self.env),
                            &mut Default::default(),
                        )
                });
            if (aliases_receiver || aliases_argument) && !reported_places.contains(&place) {
                reported_places.push(place);
                self.sink
                    .push(diagnostics::infer::aliased_writable_argument(
                        &render_place(argument),
                        argument.get_span(),
                    ));
            }
        }
    }

    fn argument_shares_storage(
        &self,
        ty: &Type,
        visiting: &mut FxHashSet<(Symbol, Vec<Type>)>,
    ) -> bool {
        use syntax::types::{build_substitution_map, substitute};
        match self.store.peel_alias(ty) {
            Type::Compound { .. } | Type::Function(_) | Type::Parameter(_) | Type::Var { .. } => {
                true
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|ty| self.argument_shares_storage(ty, visiting)),
            Type::Array { element, .. } => self.argument_shares_storage(&element, visiting),
            Type::Nominal {
                id,
                params: parameters,
                ..
            } => {
                let instance = (id.clone(), parameters.clone());
                if visiting.contains(&instance) {
                    return false;
                }

                if visiting.iter().any(|(owner, arguments)| {
                    *owner == id
                        && !arguments.iter().any(|argument| {
                            argument.contains_type(&Type::Nominal {
                                id: id.clone(),
                                params: parameters.clone(),
                                writable: false,
                            })
                        })
                }) {
                    return true;
                }
                visiting.insert(instance.clone());
                let shares = self.store.get_definition(&id).is_none_or(|definition| {
                    let map = build_substitution_map(
                        definition.body.generics().unwrap_or_default(),
                        &parameters,
                    );
                    let mut field_shares =
                        |ty: &Type| self.argument_shares_storage(&substitute(ty, &map), visiting);
                    match &definition.body {
                        DefinitionBody::Struct { fields, .. } => fields
                            .as_slice()
                            .iter()
                            .any(|field| field_shares(&field.ty)),
                        DefinitionBody::Enum { variants, .. } => variants
                            .iter()
                            .flat_map(|variant| variant.fields.iter())
                            .any(|field| field_shares(&field.ty)),
                        DefinitionBody::Interface { .. } | DefinitionBody::TypeAlias { .. } => true,
                        DefinitionBody::Value { .. } => false,
                    }
                });
                visiting.remove(&instance);
                shares
            }
            Type::Forall { body, .. } => self.argument_shares_storage(&body, visiting),
            Type::Simple(_)
            | Type::ImportNamespace(_)
            | Type::Uninferred
            | Type::Ignored
            | Type::Never
            | Type::Error
            | Type::ReceiverPlaceholder => false,
        }
    }

    fn names_no_storage(&self, arg: &Expression) -> bool {
        let definition = match arg {
            Expression::Identifier {
                resolution: IdentifierResolution::Definition { name, .. },
                ..
            } => Some(name.as_str()),
            Expression::DotAccess {
                resolution:
                    DotAccessResolution::EnumVariant { .. }
                    | DotAccessResolution::StaticMethod { .. }
                    | DotAccessResolution::InstanceMethodValue { .. },
                ..
            } => return true,
            Expression::DotAccess {
                resolution: DotAccessResolution::PackageMember { definition, .. },
                ..
            } => definition.as_ref().map(Symbol::as_str),
            _ => None,
        };
        definition.is_some_and(|name| {
            !matches!(
                self.store
                    .get_definition(name)
                    .map(|definition| &definition.body),
                Some(DefinitionBody::Value {
                    kind: ValueKind::Variable,
                    ..
                })
            )
        })
    }

    /// Verify the substring arg is a range type over `int`; emit a `Range<int>` mismatch otherwise.
    pub(super) fn validate_substring_range_arg(&mut self, arg: &Expression) {
        let store = self.store;
        let arg_ty = arg.get_type().resolve_in(&self.env);
        let arg_span = arg.get_span();
        let int_ty = Type::int();

        if let Some(peeled) = peel_to_range_type(&arg_ty, |id| self.store.get_definition(id)) {
            if let Some(inner) = peeled.get_type_params().and_then(|p| p.first()) {
                self.unify(&int_ty, inner, &arg_span);
            }
        } else {
            let expected = self.type_range(store, int_ty);
            self.unify(&expected, &arg_ty, &arg_span);
        }
        let resolved = arg.get_type().resolve_in(&self.env);
        if !arg.unwrap_parens().is_range()
            && let Some(kind) = peel_to_range_type(&resolved, |id| self.store.get_definition(id))
                .as_ref()
                .and_then(RangeKind::of)
        {
            self.facts
                .checked_bodies
                .insert_range_kind(arg.unwrap_parens().id(), kind);
        }
    }

    /// Index of the `Range` param to relax for a native-string `substring` call, or `None`.
    pub(super) fn substring_carve_out_param_idx(
        &self,
        call_kind: CallKind,
        callee: &Expression,
        parameters: &[FunctionParameter],
    ) -> Option<usize> {
        if !matches!(
            call_kind,
            CallKind::NativeMethod(NativeTypeKind::String)
                | CallKind::NativeMethodIdentifier(NativeTypeKind::String)
        ) {
            return None;
        }
        let is_substring = match callee {
            Expression::DotAccess { member, .. } => member.as_str() == "substring",
            Expression::Identifier { value, .. } => value
                .rsplit_once('.')
                .is_some_and(|(_, method)| method == "substring"),
            _ => false,
        };
        if !is_substring {
            return None;
        }
        parameters.iter().position(|param| {
            param
                .ty
                .resolve_in(&self.env)
                .get_qualified_id()
                .is_some_and(|id| id == "prelude.Range")
        })
    }
}
