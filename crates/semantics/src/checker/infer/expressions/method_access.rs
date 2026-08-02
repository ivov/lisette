use std::sync::Arc;

use crate::checker::EnvResolve;
use syntax::ast::{Expression, Span};
use syntax::program::{Definition, DefinitionBody, DotAccessResolution, ReceiverCoercion};
use syntax::types::{Symbol, Type, unqualified_name};

use super::super::addressability::check_is_non_addressable;
use super::primitives::contains_deref;
use crate::checker::infer::InferCtx;
use crate::checker::promotion::{self, Resolution};

use super::dot_access::DotAccessResolutionArgs;

impl InferCtx<'_> {
    pub(super) fn as_instance_method(
        &mut self,
        args: &DotAccessResolutionArgs,
    ) -> Option<Expression> {
        let store = self.store;

        // Array methods live on the size-erased prelude `Array` impl, reached via
        // the `method_lookup_key` bridge, with the size ignored in receiver unify.
        let (method_ty, is_exported, resolved_definition) = if let Type::Array { .. } =
            &args.deref_ty
        {
            (
                self.get_all_methods(store, &args.deref_ty)
                    .get(args.member_name)
                    .cloned()?,
                true,
                self.resolve_instance_method_definition(&args.deref_ty, args.member_name),
            )
        } else {
            if !matches!(
                args.deref_ty,
                Type::Nominal { .. } | Type::Parameter(_) | Type::Compound { .. } | Type::Simple(_)
            ) {
                return None;
            }

            let method_ty = self
                .get_all_methods(store, &args.deref_ty)
                .get(args.member_name)
                .cloned()?;

            if self.is_type_level_receiver(args.expression)
                && self.method_is_promoted(&args.deref_ty, args.member_name)
            {
                return self.as_promoted_method_expression(args, &method_ty);
            }

            let resolved_definition =
                self.check_instance_method_access(&args.deref_ty, &method_ty, args, None);

            let is_exported = self.is_dot_access_exported(&args.deref_ty, args.member_name);
            (method_ty, is_exported, resolved_definition)
        };

        let (mut method_ty, _) = self.instantiate(&method_ty);

        if !matches!(method_ty, Type::Function(_)) {
            return None;
        }

        if let Some(expression) = self.as_method_value(
            args,
            &mut method_ty,
            is_exported,
            resolved_definition.clone(),
        ) {
            return Some(expression);
        }

        if self.scopes.is_callee_context() && self.is_type_level_receiver(args.expression) {
            self.sink.push(diagnostics::infer::type_used_as_value(
                &args.expression.as_dotted_path().unwrap_or_default(),
                args.expression.get_span(),
            ));
        }

        let Type::Function(ref mut f) = method_ty else {
            unreachable!();
        };

        let f = Arc::make_mut(f);
        let receiver_ty = f.remove_receiver();
        let actual_ty = args.expression_ty;

        let receiver_coercion = self.unify_receiver_with_coercion(
            &receiver_ty,
            actual_ty,
            args.expression,
            args.member_name,
            args.span,
        );

        self.unify(args.expected_ty, &method_ty, args.span);

        Some(args.build_dot_access(
            method_ty,
            DotAccessResolution::InstanceMethod {
                is_exported,
                receiver_coercion,
                definition: resolved_definition,
            },
        ))
    }

    fn as_promoted_method_expression(
        &mut self,
        args: &DotAccessResolutionArgs,
        method_ty: &Type,
    ) -> Option<Expression> {
        let Resolution::Found(member) =
            promotion::resolve_selector(self.store, &args.deref_ty, args.member_name)
        else {
            return None;
        };

        let resolved_definition = member.declaring_type.with_segment(args.member_name);
        self.check_instance_method_access(
            &args.deref_ty,
            method_ty,
            args,
            Some(resolved_definition.clone()),
        );

        let (method_ty, _) = self.instantiate(method_ty);
        let Type::Function(f) = &method_ty else {
            return None;
        };
        let is_pointer_receiver = f
            .params
            .first()
            .is_some_and(|param| param.ty.resolve_in(&self.env).is_ref());

        let is_exported =
            self.promoted_method_is_exported(&member.declaring_type, args.member_name);
        if !is_exported {
            self.sink
                .push(diagnostics::infer::private_method_expression(*args.span));
        }

        self.unify(args.expected_ty, &method_ty, args.span);
        Some(args.build_dot_access(
            method_ty,
            DotAccessResolution::InstanceMethodValue {
                is_exported,
                is_pointer_receiver,
                definition: Some(resolved_definition),
            },
        ))
    }

    fn promoted_method_is_exported(&self, declaring_type: &Symbol, member_name: &str) -> bool {
        let store = self.store;
        let type_module = store
            .module_for_qualified_name(declaring_type)
            .unwrap_or(declaring_type.as_str());
        if type_module != self.cursor.module_id {
            return true;
        }
        let method_key = declaring_type.with_segment(member_name);
        store
            .get_definition(&method_key)
            .map(|d| d.visibility.is_public())
            .unwrap_or(false)
    }

    /// Check cross-module visibility, record usage for find-references,
    /// and warn if a UFCS method is taken as a value.
    fn check_instance_method_access(
        &mut self,
        deref_ty: &Type,
        method_ty: &Type,
        args: &DotAccessResolutionArgs,
        resolved_definition: Option<Symbol>,
    ) -> Option<Symbol> {
        let store = self.store;
        let resolved_definition = resolved_definition
            .or_else(|| self.resolve_instance_method_definition(deref_ty, args.member_name));
        if let Some(method_key) = resolved_definition.as_ref() {
            if let Some(definition_span) = self.get_definition_name_span(store, method_key) {
                self.facts.add_usage(*args.span, definition_span);
            }

            let declaring = method_key
                .without_last_segment()
                .unwrap_or(method_key.as_str());
            if matches!(deref_ty, Type::Nominal { .. })
                && self.is_foreign_type(declaring)
                && let Some(def) = store.get_definition(method_key)
                && matches!(def.body, DefinitionBody::Value { .. })
                && !def.visibility.is_public()
            {
                self.sink.push(diagnostics::infer::private_method_access(
                    args.member_name,
                    declaring,
                    store
                        .module_for_qualified_name(declaring)
                        .unwrap_or(declaring),
                    *args.span,
                ));
            }
        }

        if !self.scopes.is_callee_context()
            && (matches!(deref_ty, Type::Nominal { id, .. }
                    if self.store.is_ufcs_method(id.as_str(), args.member_name))
                || matches!(store.deep_resolve_alias(deref_ty), Type::Nominal { id, .. }
                    if self.store.is_ufcs_method(id.as_str(), args.member_name))
                || matches!(method_ty, Type::Forall { vars, .. }
                    if vars.len() > self.get_receiver_generics_count(deref_ty)))
        {
            self.sink
                .push(diagnostics::infer::taking_value_of_ufcs_method(*args.span));
        }

        resolved_definition
    }

    fn resolve_instance_method_definition(
        &self,
        receiver_type: &Type,
        method_name: &str,
    ) -> Option<Symbol> {
        let store = self.store;
        if let Type::Nominal { id, .. } = receiver_type {
            let direct = id.with_segment(method_name);
            if store.get_definition(&direct).is_some() {
                return Some(direct);
            }
            if promotion::has_direct_embed(store, receiver_type)
                && let Resolution::Found(member) =
                    promotion::resolve_selector(store, receiver_type, method_name)
            {
                let promoted = member.declaring_type.with_segment(method_name);
                if store.get_definition(&promoted).is_some() {
                    return Some(promoted);
                }
            }
        }

        let resolved = store.deep_resolve_alias(receiver_type);
        let owner = match resolved.strip_refs() {
            Type::Nominal { id, .. } => id,
            Type::Simple(kind) => Symbol::from_parts("prelude", kind.leaf_name()),
            Type::Compound { kind, .. } => Symbol::from_parts("prelude", kind.leaf_name()),
            Type::Array { .. } => Symbol::from_parts("prelude", "Array"),
            _ => return None,
        };
        let resolved_definition = owner.with_segment(method_name);
        store
            .get_definition(&resolved_definition)
            .is_some()
            .then_some(resolved_definition)
    }

    /// When a cross-module instance method is used as a value (not called),
    /// preserve the receiver in the type signature. The emitter emits Go
    /// method expression syntax (e.g., `lib.Point.Sum`).
    fn as_method_value(
        &mut self,
        args: &DotAccessResolutionArgs,
        method_ty: &mut Type,
        is_exported: bool,
        resolved_definition: Option<Symbol>,
    ) -> Option<Expression> {
        let Type::Function(f) = &*method_ty else {
            return None;
        };
        let params = &f.params;

        let is_cross_module_type_access = matches!(
            args.expression,
            Expression::DotAccess { expression: inner, .. }
                if inner.get_type().resolve_in(&self.env).as_import_namespace().is_some()
        );

        if !is_cross_module_type_access || self.scopes.is_callee_context() {
            return None;
        }

        // Don't remove self: the value type should include the receiver.
        // Still unify the receiver type with the expression type for generic resolution.
        let receiver_ty = params[0].ty.resolve_in(&self.env);
        let receiver_stripped = receiver_ty.strip_refs();
        let expression_stripped = args.expression_ty.resolve_in(&self.env).strip_refs();
        self.unify(&receiver_stripped, &expression_stripped, args.span);

        self.unify(args.expected_ty, method_ty, args.span);

        let is_pointer_receiver = matches!(method_ty, Type::Function(f) if !f.params.is_empty() && f.params[0].ty.resolve_in(&self.env).is_ref());
        Some(args.build_dot_access(
            method_ty.clone(),
            DotAccessResolution::InstanceMethodValue {
                is_exported,
                is_pointer_receiver,
                definition: resolved_definition,
            },
        ))
    }

    /// Unifies receiver type with coercion support for method calls.
    /// Matches Go's behavior: auto-address (T → Ref<T>) and auto-deref (Ref<T> → T).
    ///
    /// Returns the coercion (if any) that should be attached to the enclosing
    /// `DotAccess` expression so the emitter can apply it to the receiver.
    fn unify_receiver_with_coercion(
        &mut self,
        receiver_ty: &Type,
        actual_ty: &Type,
        receiver_expression: &Expression,
        method_name: &str,
        span: &Span,
    ) -> Option<ReceiverCoercion> {
        // Resolve to follow any type variable links before checking is_ref
        let receiver_ty = receiver_ty.resolve_in(&self.env);
        let actual_ty = actual_ty.resolve_in(&self.env);
        let receiver_is_ref = receiver_ty.is_ref();
        let actual_is_ref = actual_ty.is_ref();

        let mut coercion = None;

        match (receiver_is_ref, actual_is_ref) {
            (true, false) => {
                // Method expects Ref<T>, have T → auto-address
                if let Some(kind) =
                    check_is_non_addressable(receiver_expression, &self.env, self.store)
                {
                    self.sink
                        .push(diagnostics::infer::cannot_auto_address_receiver(
                            kind,
                            method_name,
                            &receiver_ty,
                            &actual_ty,
                            *span,
                        ));
                } else {
                    coercion = Some(ReceiverCoercion::AutoAddress);
                    self.check_auto_address_mutation(receiver_expression, method_name, span);
                }
                // Unify inner types: T with T (from Ref<T>)
                if let Some(inner) = receiver_ty.inner() {
                    self.unify(&inner, &actual_ty, span);
                }
            }
            (false, true) => {
                // Method expects T, have Ref<T> → auto-deref
                coercion = Some(ReceiverCoercion::AutoDeref);
                // Unify inner types: T with T (from Ref<T>)
                if let Some(inner) = actual_ty.inner() {
                    self.unify(&receiver_ty, &inner, span);
                }
            }
            (true, true) => {
                // Both are refs, normal unification (handles same depth)
                // Note: Multi-level mismatches (Ref<Ref<T>> vs Ref<T>) will fail in unify
                self.unify(&receiver_ty, &actual_ty, span);
            }
            (false, false) => {
                // Neither is ref, normal unification
                self.unify(&receiver_ty, &actual_ty, span);
            }
        }

        coercion
    }

    /// When auto-addressing a receiver (T → Ref<T>), verify the binding
    /// is declared `let mut`, since the Ref<T> method may mutate it.
    fn check_auto_address_mutation(
        &mut self,
        receiver_expression: &Expression,
        _method_name: &str,
        span: &Span,
    ) {
        let store = self.store;
        // Ref<T> methods can mutate, require `let mut` on the receiver binding,
        // unless the receiver chain contains a deref (mutation goes through pointer).
        let Some(var_name) = receiver_expression.get_var_name() else {
            return;
        };

        if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
            self.facts.mark_alias_mutated(binding_id);
        }
        let is_deref = contains_deref(receiver_expression);
        let binding_is_ref = self
            .scopes
            .lookup_value(&var_name)
            .map(|t| t.resolve_in(&self.env).is_ref())
            .unwrap_or(false);
        if !is_deref
            && !binding_is_ref
            && !self.scopes.lookup_mutable(&var_name)
            && self.imports.namespace(&var_name).is_none()
        {
            let self_type_name = if var_name == "self" {
                self.lookup_type(store, "self")
                    .and_then(|t| t.get_name().map(str::to_owned))
            } else {
                None
            };
            let is_pattern_binding = self
                .scopes
                .lookup_binding_id(&var_name)
                .and_then(|id| self.facts.bindings.get(&id))
                .is_some_and(|b| b.kind.is_pattern_position());
            let is_const = self.is_const_var(store, &var_name);
            self.sink.push(diagnostics::infer::disallowed_mutation(
                &var_name,
                *span,
                self_type_name.as_deref(),
                is_pattern_binding,
                is_const,
            ));
        }
    }

    fn get_receiver_generics_count(&self, receiver_ty: &Type) -> usize {
        let store = self.store;
        let lookup_id: Symbol = match receiver_ty {
            Type::Nominal { id, .. } => id.clone(),
            Type::Compound { kind, .. } => Symbol::from_parts("prelude", kind.leaf_name()),
            _ => return 0,
        };

        match store.get_definition(&lookup_id).map(|d| &d.body) {
            Some(DefinitionBody::Struct { generics, .. }) => generics.len(),
            Some(DefinitionBody::TypeAlias { generics, .. }) => generics.len(),
            Some(DefinitionBody::Enum { generics, .. }) => generics.len(),
            _ => 0,
        }
    }

    pub(super) fn as_static_method(
        &mut self,
        args: &DotAccessResolutionArgs,
    ) -> Option<Expression> {
        let store = self.store;
        let id = match &args.deref_ty {
            Type::Function(f) => {
                if let Type::Nominal { id, .. } = store.peel_alias(&f.return_type) {
                    id
                } else {
                    return None;
                }
            }
            ty => match store.peel_alias(ty) {
                Type::Nominal { id, .. } => {
                    if let Some(def) = store.get_definition(&id)
                        && matches!(def.body, DefinitionBody::Enum { .. })
                    {
                        let is_type_access = matches!(
                            args.expression,
                            Expression::DotAccess { expression, .. }
                                if expression.get_type().resolve_in(&self.env).as_import_namespace().is_some()
                        );
                        if !is_type_access {
                            return None;
                        }
                    }
                    id
                }
                Type::Simple(kind) => Symbol::from_parts("prelude", kind.leaf_name()),
                Type::Compound { kind, .. } => Symbol::from_parts("prelude", kind.leaf_name()),
                _ => return None,
            },
        };

        if self
            .get_all_methods(store, &args.deref_ty)
            .contains_key(args.member_name)
        {
            return None;
        }

        let method_qualified_name = id.with_segment(args.member_name);
        let method_definition = store.get_definition(&method_qualified_name)?;

        let Definition {
            ty: method_ty,
            name_span,
            visibility,
            body: DefinitionBody::Value { .. },
            ..
        } = method_definition
        else {
            return None;
        };

        let method_ty = method_ty.clone();
        let name_span = *name_span;
        let is_public = visibility.is_public();
        let type_simple_name = unqualified_name(&id);

        if !self.is_type_level_receiver(args.expression) {
            let member_len = args.member_name.len() as u32;
            let member_span = Span {
                file_id: args.span.file_id,
                byte_offset: args.span.byte_offset + args.span.byte_length - member_len,
                byte_length: member_len,
            };
            self.sink
                .push(diagnostics::infer::static_method_called_on_instance(
                    args.member_name,
                    type_simple_name,
                    member_span,
                ));
        }

        if self.is_foreign_type(&id) && !is_public {
            self.sink.push(diagnostics::infer::private_method_access(
                args.member_name,
                type_simple_name,
                store.module_for_qualified_name(&id).unwrap_or(&id),
                *args.span,
            ));
        }

        if let Some(definition_span) = name_span {
            self.facts.add_usage(*args.span, definition_span);
        }

        let type_name_len = type_simple_name.len() as u32;
        self.track_name_usage(store, &id, args.span, type_name_len);

        let (method_ty, _) = self.instantiate(&method_ty);

        self.unify(args.expected_ty, &method_ty, args.span);

        let type_module = store.module_for_qualified_name(&id).unwrap_or(&id);
        let is_cross_module = type_module != self.cursor.module_id;
        let is_exported = is_public || is_cross_module;
        Some(args.build_dot_access(
            method_ty,
            DotAccessResolution::StaticMethod {
                is_exported,
                definition: method_qualified_name,
            },
        ))
    }

    fn is_dot_access_exported(&self, deref_ty: &Type, member_name: &str) -> bool {
        let store = self.store;
        let Type::Nominal { id, .. } = deref_ty.strip_refs() else {
            // Type parameters (bounded generics), can't determine module,
            // fall back to false; the emitter will check method_needs_export.
            return false;
        };
        let type_module = store.module_for_qualified_name(&id).unwrap_or(&id);
        let is_cross_module = type_module != self.cursor.module_id;

        if is_cross_module {
            return true;
        }

        let method_key = id.with_segment(member_name);
        store
            .get_definition(&method_key)
            .map(|d| d.visibility.is_public())
            .unwrap_or(false)
    }
}
