use crate::checker::EnvResolve;
use syntax::ast::{Expression, Span, StructFields};
use syntax::program::{CallKind, Definition, DefinitionBody, NativeTypeKind};
use syntax::types::{FunctionParameter, Symbol, Type, peel_to_range_type};

use crate::checker::infer::InferCtx;
use syntax::program::AliasKind;

impl InferCtx<'_> {
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
                let peeled = store.deep_resolve_alias(&receiver_ty);

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
            Expression::Identifier { value, .. } => {
                let qualified = self.qualify_name(value);
                let definition = store.get_definition(&qualified);
                if definition.is_none() && value == "assert_type" {
                    return CallKind::AssertType;
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
        match expression {
            Expression::Identifier { value, .. } => value == "panic",
            _ => false,
        }
    }

    /// `Map.delete` and `Slice.copy_from` modify the receiver in place, so it
    /// needs `mut`. `append` is pure (it returns a new slice and needs no
    /// `mut`): growing in place is the reassignment `s = s.append(x)`, checked
    /// as an ordinary assignment, including the rejection of writing back to a
    /// non-addressable map-value field.
    pub(super) fn check_native_mutating_call(&mut self, callee: &Expression, span: &Span) {
        let store = self.store;
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee
        else {
            return;
        };
        let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();

        let is_mutating = match receiver_ty.get_name() {
            Some("Map") => member == "delete",
            Some("Slice") => member == "copy_from",
            _ => false,
        };
        if !is_mutating {
            return;
        }
        if let Some(var_name) = receiver.get_var_name()
            && let Some(binding_id) = self.scopes.lookup_binding_id(&var_name)
        {
            self.facts.mark_alias_mutated(binding_id);
        }
        // The write goes through the receiver handle, so its type must be writable.
        let governing = store.peel_alias(&receiver_ty);
        if !governing.is_writable() && !governing.is_error() {
            let receiver_place = super::aliasing::render_place(receiver);
            self.sink.push(diagnostics::infer::write_through_read_only(
                &receiver_place,
                &receiver_place,
                &governing.to_string(),
                *span,
                None,
            ));
        }
    }

    pub(super) fn check_aliased_writable_arguments(
        &mut self,
        args: &[Expression],
        parameters: &[FunctionParameter],
        variadic: Option<&FunctionParameter>,
        writable_receiver: Option<String>,
    ) {
        let places: Vec<Option<String>> = args
            .iter()
            .map(|arg| super::aliasing::place_key(arg.unwrap_parens()))
            .collect();

        let writable_positions: Vec<bool> = (0..args.len())
            .map(|i| {
                parameters
                    .get(i)
                    .or(if i >= parameters.len() {
                        variadic
                    } else {
                        None
                    })
                    .is_some_and(|param| {
                        self.store
                            .parameter_grants_write(&param.ty.resolve_in(&self.env))
                    })
            })
            .collect();

        for (i, arg) in args.iter().enumerate() {
            let writable_position = writable_positions[i];
            let Some(place) = places[i].as_deref() else {
                continue;
            };
            if writable_position
                && let Some(binding_id) = arg
                    .get_var_name()
                    .and_then(|name| self.scopes.lookup_binding_id(&name))
            {
                self.facts.mark_alias_mutated(binding_id);
            }
            let aliases_receiver = writable_receiver.as_deref() == Some(place);
            let aliases_argument = writable_position
                && places
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && other.as_deref() == Some(place));
            if aliases_receiver || aliases_argument {
                self.sink
                    .push(diagnostics::infer::aliased_writable_argument(
                        place,
                        arg.get_span(),
                    ));
            }
        }
    }

    /// Verify the substring arg is a range type over `int`; emit a `Range<int>` mismatch otherwise.
    pub(super) fn validate_substring_range_arg(&mut self, arg: &Expression) {
        let store = self.store;
        let arg_ty = arg.get_type().resolve_in(&self.env);
        let arg_span = arg.get_span();
        let int_ty = self.type_int();

        if let Some(peeled) = peel_to_range_type(&arg_ty, |id| self.store.get_definition(id)) {
            if let Some(inner) = peeled.get_type_params().and_then(|p| p.first()) {
                self.unify(&int_ty, inner, &arg_span);
            }
        } else {
            let expected = self.type_range(store, int_ty);
            self.unify(&expected, &arg_ty, &arg_span);
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
                .get_name()
                .is_some_and(|n| n == "Range")
        })
    }
}
