use crate::checker::EnvResolve;
use crate::store::Store;
use ecow::EcoString;
use syntax::ast::{Expression, Span, StructKind};
use syntax::program::{
    Definition, DefinitionBody, DotAccessKind, NativeTypeKind, ReceiverCoercion,
};
use syntax::types::{Symbol, Type, module_part, substitute, unqualified_name};

use super::super::TaskState;
use super::super::addressability::check_is_non_addressable;
use super::primitives::contains_deref;

impl TaskState<'_> {
    pub(super) fn infer_dot_access_or_qualified_path(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        member: EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        {
            let mut inner = &*expression;
            while let Expression::Paren { expression: e, .. } = inner {
                inner = e;
            }
            if !std::ptr::eq(inner, &*expression)
                && let Some(path) = inner.as_dotted_path()
                && inner.root_identifier().is_some_and(|root| {
                    self.lookup_qualified_name(store, root).is_some()
                        || self.imports.imported_modules.contains_key(root)
                })
            {
                self.sink.push(diagnostics::infer::parenthesized_qualifier(
                    &path,
                    &member,
                    expression.get_span(),
                ));
                return Expression::DotAccess {
                    expression,
                    member,
                    ty: expected_ty.clone(),
                    span,
                    dot_access_kind: None,
                    receiver_coercion: None,
                };
            }
        }

        if let Some(root) = expression.root_identifier()
            && let Some(qualified_root) = self.lookup_qualified_name(store, root)
            && let Some(base) = expression.as_dotted_path()
        {
            let path = format!("{}.{}", base, member);
            if self.lookup_type(store, &path).is_some() {
                self.track_name_usage(store, &qualified_root, &span, root.len() as u32);
                return self.infer_expression(
                    store,
                    Expression::Identifier {
                        value: path.into(),
                        ty: Type::uninferred(),
                        span,
                        binding_id: None,
                        qualified: None,
                    },
                    expected_ty,
                );
            }

            let alias_target = store
                .get_definition(&qualified_root)
                .and_then(|definition| {
                    if let DefinitionBody::TypeAlias { .. } = &definition.body {
                        let underlying = definition.ty.unwrap_forall();
                        match underlying {
                            Type::Nominal { id, params, .. }
                                if params.is_empty() && id.as_str() != qualified_root.as_str() =>
                            {
                                return Some(id.to_string());
                            }
                            Type::Simple(kind) => {
                                return Some(format!("prelude.{}", kind.leaf_name()));
                            }
                            Type::Compound { kind, args } if args.is_empty() => {
                                return Some(format!("prelude.{}", kind.leaf_name()));
                            }
                            _ => {}
                        }
                    }
                    None
                });

            if let Some(resolved_id) = alias_target {
                let mut paths = Vec::with_capacity(2);
                let short_name = unqualified_name(&resolved_id);
                if short_name != resolved_id {
                    paths.push(format!("{}.{}", short_name, member));
                }
                paths.push(format!("{}.{}", resolved_id, member));

                for path in paths {
                    if self.lookup_type(store, &path).is_some() {
                        return self.infer_expression(
                            store,
                            Expression::Identifier {
                                value: path.into(),
                                ty: Type::uninferred(),
                                span,
                                binding_id: None,
                                qualified: None,
                            },
                            expected_ty,
                        );
                    }
                }
            }
        }

        if let Some(root) = expression.root_identifier()
            && let Some(qualified_root) = self.lookup_qualified_name(store, root)
            && let Some(def) = store.get_definition(&qualified_root)
            && matches!(def.body, DefinitionBody::TypeAlias { .. })
        {
            let alias_ty = &def.ty;
            let underlying = alias_ty.unwrap_forall();
            let is_generic = matches!(alias_ty, Type::Forall { .. })
                || matches!(underlying, Type::Nominal { params, .. } if !params.is_empty());
            if is_generic {
                let type_name = if let Type::Nominal { id, .. } = underlying {
                    unqualified_name(id).to_string()
                } else {
                    "the original type".to_string()
                };
                self.sink.push(diagnostics::infer::type_alias_as_qualifier(
                    root,
                    &type_name,
                    &member,
                    expression.get_span(),
                ));
                return Expression::DotAccess {
                    expression,
                    member,
                    ty: expected_ty.clone(),
                    span,
                    dot_access_kind: None,
                    receiver_coercion: None,
                };
            }
        }

        self.infer_dot_access(store, expression, member, span, expected_ty)
    }
}

struct DotAccessResolutionArgs<'a> {
    expression: &'a Expression,
    expression_ty: &'a Type,
    /// `expression_ty.strip_refs()`, precomputed once per dot-access
    /// resolution so each `as_*` resolver doesn't recompute it.
    deref_ty: Type,
    member_name: &'a str,
    span: &'a Span,
    expected_ty: &'a Type,
}

impl DotAccessResolutionArgs<'_> {
    fn build_dot_access(
        &self,
        ty: Type,
        kind: Option<DotAccessKind>,
        receiver_coercion: Option<ReceiverCoercion>,
    ) -> Expression {
        Expression::DotAccess {
            expression: self.expression.clone().into(),
            member: self.member_name.into(),
            ty,
            span: *self.span,
            dot_access_kind: kind,
            receiver_coercion,
        }
    }
}

impl TaskState<'_> {
    pub(super) fn infer_dot_access(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        member: EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let expression_ty = self.new_type_var();
        let prior_dot_access_base = self.scopes.set_dot_access_base(true);
        let new_expression = self.infer_expression(store, *expression, &expression_ty);
        self.scopes.set_dot_access_base(prior_dot_access_base);
        let resolved_expression_ty = expression_ty.resolve_in(&self.env);

        let deref_ty = resolved_expression_ty.strip_refs();

        let args = DotAccessResolutionArgs {
            expression: &new_expression,
            expression_ty: &resolved_expression_ty,
            deref_ty,
            member_name: &member,
            span: &span,
            expected_ty,
        };

        if resolved_expression_ty.is_error() || resolved_expression_ty.is_variable() {
            self.unify(store, expected_ty, &Type::Error, &span);
            return args.build_dot_access(Type::Error, None, None);
        }

        let resolved = self
            .as_struct_field(store, &args)
            .or_else(|| self.as_tuple_element(store, &args))
            .or_else(|| self.as_module_member(store, &args))
            .or_else(|| self.as_enum_variant(store, &args))
            .or_else(|| self.as_instance_method(store, &args))
            .or_else(|| self.as_static_method(store, &args));

        if let Some((expression, _kind)) = resolved {
            if (member.as_str() == "append" || member.as_str() == "extend")
                && resolved_expression_ty.is_ref()
                && args.deref_ty.has_name("Slice")
            {
                self.sink.push(diagnostics::infer::ref_slice_append(span));
            }
            if !self.scopes.is_callee_context()
                && matches!(
                    expression.get_type().resolve_in(&self.env),
                    Type::Function { .. } | Type::Forall { .. }
                )
                && NativeTypeKind::from_type(&resolved_expression_ty).is_some()
            {
                self.sink
                    .push(diagnostics::infer::native_method_value(&member, span));
            }
            return expression;
        }

        let available_members = self.get_available_member_names(store, &resolved_expression_ty);
        let unwrap_hint = self.compute_unwrap_hint(store, &resolved_expression_ty, &member);
        self.sink.push(diagnostics::infer::member_not_found(
            &resolved_expression_ty,
            &member,
            span,
            if available_members.is_empty() {
                None
            } else {
                Some(&available_members)
            },
            unwrap_hint,
        ));

        args.build_dot_access(Type::Error, None, None)
    }

    /// Whether a type's owning module is foreign (not current, prelude, or Go stdlib).
    /// Used to gate cross-module visibility checks on methods.
    fn is_foreign_type(&self, type_id: &str) -> bool {
        let type_module = module_part(type_id);
        type_module != self.cursor.module_id
            && type_module != "prelude"
            && !type_module.starts_with("go:")
    }

    fn is_type_level_receiver(&self, store: &Store, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier {
                binding_id: None,
                qualified: Some(qname),
                ..
            } => store
                .get_definition(qname)
                .is_some_and(Definition::is_type_definition),
            Expression::DotAccess {
                expression: inner, ..
            } => inner
                .get_type()
                .shallow_resolve_in(&self.env)
                .as_import_namespace()
                .is_some(),
            _ => false,
        }
    }

    fn get_available_member_names(&self, store: &Store, ty: &Type) -> Vec<String> {
        let deref_ty = ty.strip_refs();
        let mut names = Vec::new();

        if let Type::Nominal { .. } = deref_ty {
            let qualified_name = deref_ty.get_qualified_name();
            if let Some(fields) = store.fields_of(&qualified_name) {
                names.extend(fields.iter().map(|f| f.name.to_string()));
            }
        }

        let methods = self.get_all_methods(store, &deref_ty);
        names.extend(methods.into_keys().map(|k| k.to_string()));

        names
    }

    fn compute_unwrap_hint(
        &self,
        store: &Store,
        ty: &Type,
        member: &str,
    ) -> Option<diagnostics::infer::UnwrapHint> {
        let wrapper = if ty.is_option() {
            diagnostics::infer::UnwrapWrapper::Option
        } else if ty.is_result() {
            diagnostics::infer::UnwrapWrapper::Result
        } else {
            return None;
        };

        let inner = ty.inner()?.strip_refs();
        if self.has_member(store, &inner, member) {
            Some(diagnostics::infer::UnwrapHint {
                wrapper,
                inner_ty: inner,
            })
        } else {
            None
        }
    }

    fn has_member(&self, store: &Store, ty: &Type, member: &str) -> bool {
        let deref_ty = ty.strip_refs();

        if let Type::Nominal { .. } = deref_ty
            && let Some(fields) = store.fields_of(&deref_ty.get_qualified_name())
            && fields.iter().any(|f| f.name == member)
        {
            return true;
        }

        self.get_all_methods(store, &deref_ty).contains_key(member)
    }

    fn as_struct_field(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let Type::Nominal { .. } = &args.deref_ty else {
            return None;
        };

        let qualified_name = args.deref_ty.get_qualified_name();

        let struct_name = {
            let mut name = qualified_name.clone();
            let mut seen = Vec::new();
            loop {
                if seen.contains(&name) {
                    break;
                }
                seen.push(name.clone());
                let new_name = match store.get_definition(&name) {
                    Some(Definition {
                        ty,
                        body: DefinitionBody::TypeAlias { .. },
                        ..
                    }) => {
                        if let Type::Nominal { id, .. } = ty.unwrap_forall()
                            && id.as_str() != name.as_str()
                        {
                            id.clone()
                        } else {
                            break;
                        }
                    }
                    _ => break,
                };
                name = new_name;
            }
            name
        };

        let Some(Definition {
            ty: struct_type,
            body:
                DefinitionBody::Struct {
                    fields: struct_fields,
                    kind: struct_kind,
                    generics,
                    ..
                },
            ..
        }) = store.get_definition(&struct_name)
        else {
            return None;
        };

        let struct_kind = *struct_kind;
        let struct_type = struct_type.clone();
        let is_newtype =
            struct_kind == StructKind::Tuple && struct_fields.len() == 1 && generics.is_empty();

        let field_name = if struct_kind == StructKind::Tuple {
            if let Ok(index) = args.member_name.parse::<usize>() {
                format!("_{}", index)
            } else {
                args.member_name.to_string()
            }
        } else {
            args.member_name.to_string()
        };

        let field = struct_fields.iter().find(|f| f.name == field_name)?;

        let field_type = field.ty.clone();
        let field_is_pub = field.visibility.is_public();

        self.facts.add_usage(*args.span, field.name_span);

        let struct_module = module_part(&struct_name);
        let is_cross_module = struct_module != self.cursor.module_id;

        if is_cross_module && !field_is_pub {
            self.sink.push(diagnostics::infer::private_field_access(
                args.member_name,
                &qualified_name,
                *args.span,
            ));
        }

        let (struct_ty, map) = self.instantiate(&struct_type);
        let field_ty = substitute(&field_type, &map);

        self.unify(store, &args.deref_ty, &struct_ty, args.span);
        self.unify(store, args.expected_ty, &field_ty, args.span);

        let is_exported = field_is_pub || is_cross_module;
        let kind = if struct_kind == StructKind::Tuple {
            DotAccessKind::TupleStructField { is_newtype }
        } else {
            DotAccessKind::StructField { is_exported }
        };

        Some((args.build_dot_access(field_ty, Some(kind), None), kind))
    }

    fn as_tuple_element(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let index: usize = args.member_name.parse().ok()?;

        let Type::Tuple(elements) = &args.deref_ty else {
            return None;
        };

        if index >= elements.len() {
            return None;
        }

        let element_ty = elements[index].clone();
        self.unify(store, args.expected_ty, &element_ty, args.span);

        let kind = DotAccessKind::TupleElement;
        Some((args.build_dot_access(element_ty, Some(kind), None), kind))
    }

    fn as_module_member(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let type_name = args.deref_ty.get_name()?;

        // Look up by type-derived name first (works for non-aliased imports).
        // For aliased imports (e.g. `import u "utils"`), the map key is "u" but
        // the type name is "utils", so fall back to matching by import module id.
        let (module_fields, module_ty) = self
            .imports
            .imported_modules
            .get(type_name)
            .cloned()
            .or_else(|| {
                let module_id = args.deref_ty.as_import_namespace()?;
                self.imports
                    .imported_modules
                    .values()
                    .find(|(_, ty)| ty.as_import_namespace() == Some(module_id))
                    .cloned()
            })?;

        let kind = DotAccessKind::ModuleMember;

        let Some(member_type) = module_fields
            .iter()
            .find(|f| f.name == args.member_name)
            .map(|f| f.ty.clone())
        else {
            self.sink
                .push(diagnostics::infer::function_or_value_not_found_in_module(
                    args.member_name,
                    *args.span,
                ));
            return Some((args.build_dot_access(Type::Error, Some(kind), None), kind));
        };

        if let Some(module_id) = module_ty.as_import_namespace() {
            let qualified_name = Symbol::from_parts(module_id, args.member_name);
            if let Some(definition_span) = self.get_definition_name_span(store, &qualified_name) {
                self.facts.add_usage(*args.span, definition_span);
            }

            // Reject cross-module tuple-struct constructors used as values
            if !self.scopes.is_callee_context()
                && matches!(
                    store.get_definition(&qualified_name).map(|d| &d.body),
                    Some(DefinitionBody::Struct {
                        kind: StructKind::Tuple,
                        ..
                    })
                )
            {
                let display_name = format!("{}.{}", type_name, args.member_name);
                self.sink.push(diagnostics::infer::native_constructor_value(
                    &display_name,
                    *args.span,
                ));
            }

            if !self.scopes.is_callee_context()
                && !self.scopes.is_dot_access_base()
                && matches!(
                    store.get_definition(&qualified_name).map(|d| &d.body),
                    Some(DefinitionBody::Struct {
                        kind: StructKind::Record,
                        ..
                    })
                )
            {
                let display_name = format!("{}.{}", type_name, args.member_name);
                self.sink.push(diagnostics::infer::record_struct_value(
                    &display_name,
                    *args.span,
                ));
            }
        }

        let (module_ty, _) = self.instantiate(&module_ty);
        let (member_ty, _) = self.instantiate(&member_type);

        self.unify(store, &args.deref_ty, &module_ty, args.span);
        self.unify(store, args.expected_ty, &member_ty, args.span);

        Some((args.build_dot_access(member_ty, Some(kind), None), kind))
    }

    fn as_instance_method(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
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

        self.check_instance_method_access(store, &args.deref_ty, &method_ty, args);

        let is_exported = self.is_dot_access_exported(store, &args.deref_ty, args.member_name);
        let kind = DotAccessKind::InstanceMethod { is_exported };

        let (mut method_ty, _) = self.instantiate(&method_ty);

        if !matches!(method_ty, Type::Function { .. }) {
            return None;
        }

        if let Some((expression, value_kind)) =
            self.as_method_value(store, args, &mut method_ty, is_exported)
        {
            return Some((expression, value_kind));
        }

        let Type::Function {
            ref mut params,
            ref mut param_mutability,
            ..
        } = method_ty
        else {
            unreachable!();
        };

        let receiver_ty = params.remove(0);
        if !param_mutability.is_empty() {
            param_mutability.remove(0);
        }
        let actual_ty = args.expression_ty;

        let receiver_coercion = self.unify_receiver_with_coercion(
            store,
            &receiver_ty,
            actual_ty,
            args.expression,
            args.member_name,
            args.span,
        );

        self.unify(store, args.expected_ty, &method_ty, args.span);

        Some((
            args.build_dot_access(method_ty, Some(kind), receiver_coercion),
            kind,
        ))
    }

    /// Check cross-module visibility, record usage for find-references,
    /// and warn if a UFCS method is taken as a value.
    fn check_instance_method_access(
        &mut self,
        store: &Store,
        deref_ty: &Type,
        method_ty: &Type,
        args: &DotAccessResolutionArgs,
    ) {
        if let Type::Nominal { .. } = deref_ty {
            let qualified_name = deref_ty.get_qualified_name();
            let method_key = qualified_name.with_segment(args.member_name);

            if let Some(definition_span) = self.get_definition_name_span(store, &method_key) {
                self.facts.add_usage(*args.span, definition_span);
            }

            if self.is_foreign_type(&qualified_name)
                && let Some(def) = store.get_definition(&method_key)
                && matches!(def.body, DefinitionBody::Value { .. })
                && !def.visibility.is_public()
            {
                self.sink.push(diagnostics::infer::private_method_access(
                    args.member_name,
                    &qualified_name,
                    *args.span,
                ));
            }
        }

        if !self.scopes.is_callee_context()
            && let Type::Forall { vars, .. } = method_ty
            && vars.len() > self.get_receiver_generics_count(store, deref_ty)
        {
            self.sink
                .push(diagnostics::infer::taking_value_of_ufcs_method(*args.span));
        }
    }

    /// When a cross-module instance method is used as a value (not called),
    /// preserve the receiver in the type signature. The emitter emits Go
    /// method expression syntax (e.g., `lib.Point.Sum`).
    fn as_method_value(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
        method_ty: &mut Type,
        is_exported: bool,
    ) -> Option<(Expression, DotAccessKind)> {
        let Type::Function { params, .. } = &*method_ty else {
            return None;
        };

        let is_cross_module_type_access = matches!(
            args.expression,
            Expression::DotAccess { expression: inner, .. }
                if inner.get_type().resolve_in(&self.env).as_import_namespace().is_some()
        );

        if !is_cross_module_type_access || self.scopes.is_callee_context() {
            return None;
        }

        // Don't remove self — the value type should include the receiver.
        // Still unify the receiver type with the expression type for generic resolution.
        let receiver_ty = params[0].resolve_in(&self.env);
        let receiver_stripped = receiver_ty.strip_refs();
        let expression_stripped = args.expression_ty.resolve_in(&self.env).strip_refs();
        self.unify(store, &receiver_stripped, &expression_stripped, args.span);

        self.unify(store, args.expected_ty, method_ty, args.span);

        let is_pointer_receiver = matches!(method_ty, Type::Function { params, .. } if !params.is_empty() && params[0].resolve_in(&self.env).is_ref());
        let value_kind = DotAccessKind::InstanceMethodValue {
            is_exported,
            is_pointer_receiver,
        };

        Some((
            args.build_dot_access(method_ty.clone(), Some(value_kind), None),
            value_kind,
        ))
    }

    /// Unifies receiver type with coercion support for method calls.
    /// Matches Go's behavior: auto-address (T → Ref<T>) and auto-deref (Ref<T> → T).
    ///
    /// Returns the coercion (if any) that should be attached to the enclosing
    /// `DotAccess` expression so the emitter can apply it to the receiver.
    fn unify_receiver_with_coercion(
        &mut self,
        store: &Store,
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
                if let Some(kind) = check_is_non_addressable(receiver_expression, &self.env) {
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
                    self.check_auto_address_mutation(store, receiver_expression, method_name, span);
                }
                // Unify inner types: T with T (from Ref<T>)
                if let Some(inner) = receiver_ty.inner() {
                    self.unify(store, &inner, &actual_ty, span);
                }
            }
            (false, true) => {
                // Method expects T, have Ref<T> → auto-deref
                coercion = Some(ReceiverCoercion::AutoDeref);
                // Unify inner types: T with T (from Ref<T>)
                if let Some(inner) = actual_ty.inner() {
                    self.unify(store, &receiver_ty, &inner, span);
                }
            }
            (true, true) => {
                // Both are refs — normal unification (handles same depth)
                // Note: Multi-level mismatches (Ref<Ref<T>> vs Ref<T>) will fail in unify
                self.unify(store, &receiver_ty, &actual_ty, span);
            }
            (false, false) => {
                // Neither is ref — normal unification
                self.unify(store, &receiver_ty, &actual_ty, span);
            }
        }

        coercion
    }

    /// When auto-addressing a receiver (T → Ref<T>), verify the binding
    /// is declared `let mut`, since the Ref<T> method may mutate it.
    fn check_auto_address_mutation(
        &mut self,
        store: &Store,
        receiver_expression: &Expression,
        _method_name: &str,
        span: &Span,
    ) {
        // Ref<T> methods can mutate — require `let mut` on the receiver binding,
        // unless the receiver chain contains a deref (mutation goes through pointer).
        let Some(var_name) = receiver_expression.get_var_name() else {
            return;
        };

        if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
            self.facts.mark_mutated(binding_id);
        }
        let is_deref = contains_deref(receiver_expression);
        let binding_is_ref = self
            .scopes
            .lookup_value(&var_name)
            .map(|t| t.resolve_in(&self.env).is_ref())
            .unwrap_or(false);
        if !is_deref && !binding_is_ref && !self.scopes.lookup_mutable(&var_name) {
            let self_type_name = if var_name == "self" {
                self.lookup_type(store, "self")
                    .and_then(|t| t.get_name().map(str::to_owned))
            } else {
                None
            };
            let is_match_arm = self
                .scopes
                .lookup_binding_id(&var_name)
                .and_then(|id| self.facts.bindings.get(&id))
                .is_some_and(|b| b.kind.is_match_arm());
            let is_const = self.is_const_var(store, &var_name);
            self.sink.push(diagnostics::infer::disallowed_mutation(
                &var_name,
                *span,
                self_type_name.as_deref(),
                is_match_arm,
                is_const,
            ));
        }
    }

    pub(crate) fn get_receiver_generics_count(&self, store: &Store, receiver_ty: &Type) -> usize {
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

    fn as_enum_variant(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let id = match &args.deref_ty {
            Type::Nominal { id, .. } => id.clone(),
            Type::Function { return_type, .. } => {
                if let Type::Nominal { id, .. } = return_type.as_ref() {
                    id.clone()
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        let definition = store.get_definition(&id)?;

        let (is_enum_variant, kind) = match &definition.body {
            DefinitionBody::Enum { variants, .. } => (
                variants.iter().any(|v| v.name == args.member_name),
                DotAccessKind::EnumVariant,
            ),
            DefinitionBody::ValueEnum { variants, .. } => (
                variants.iter().any(|v| v.name == args.member_name),
                DotAccessKind::ValueEnumVariant,
            ),
            _ => return None,
        };

        if !is_enum_variant {
            return None;
        }

        if let DefinitionBody::ValueEnum { methods, .. } = &definition.body
            && methods.contains_key(args.member_name)
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

        let variant_qualified_name = id.with_segment(args.member_name);
        let variant_definition = store.get_definition(&variant_qualified_name)?;

        let Definition {
            ty: variant_ty,
            visibility,
            name_span,
            body: DefinitionBody::Value { .. },
            ..
        } = variant_definition
        else {
            return None;
        };

        let is_foreign = self.is_foreign_type(&id);
        if is_foreign && !visibility.is_public() {
            return None;
        }

        let name_span = *name_span;
        let variant_ty = variant_ty.clone();
        if let Some(definition_span) = name_span {
            self.facts.add_usage(*args.span, definition_span);
        }

        let (variant_ty, _) = self.instantiate(&variant_ty);
        self.unify(store, args.expected_ty, &variant_ty, args.span);

        Some((args.build_dot_access(variant_ty, Some(kind), None), kind))
    }

    fn as_static_method(
        &mut self,
        store: &Store,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let id = match &args.deref_ty {
            Type::Function { return_type, .. } => {
                if let Type::Nominal { id, .. } = return_type.as_ref() {
                    id.clone()
                } else {
                    return None;
                }
            }
            Type::Nominal { id, .. } => {
                // For enums with Constructor type, we need to distinguish between:
                // - Type access (e.g., `module.Color.default()`) - ALLOW
                // - Value access (e.g., `c.new()` where c is a Color value) - REJECT
                //
                // Type access comes through DotAccess on a module import.
                // Value access comes through an Identifier or other expression.
                if let Some(def) = store.get_definition(id)
                    && matches!(
                        def.body,
                        DefinitionBody::Enum { .. } | DefinitionBody::ValueEnum { .. }
                    )
                {
                    // Check if expression is a module member access (type-level access)
                    let is_type_access = matches!(
                        args.expression,
                        Expression::DotAccess { expression, .. }
                            if expression.get_type().resolve_in(&self.env).as_import_namespace().is_some()
                    );
                    if !is_type_access {
                        return None;
                    }
                }
                id.clone()
            }
            Type::Simple(kind) => Symbol::from_parts("prelude", kind.leaf_name()),
            Type::Compound { kind, .. } => Symbol::from_parts("prelude", kind.leaf_name()),
            _ => return None,
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

        if !self.is_type_level_receiver(store, args.expression) {
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
                *args.span,
            ));
        }

        if let Some(definition_span) = name_span {
            self.facts.add_usage(*args.span, definition_span);
        }

        let type_name_len = type_simple_name.len() as u32;
        self.track_name_usage(store, &id, args.span, type_name_len);

        let (method_ty, _) = self.instantiate(&method_ty);

        self.unify(store, args.expected_ty, &method_ty, args.span);

        let type_module = module_part(&id);
        let is_cross_module = type_module != self.cursor.module_id;
        let is_exported = is_public || is_cross_module;
        let kind = DotAccessKind::StaticMethod { is_exported };

        Some((args.build_dot_access(method_ty, Some(kind), None), kind))
    }

    fn is_dot_access_exported(&self, store: &Store, deref_ty: &Type, member_name: &str) -> bool {
        let Type::Nominal { id, .. } = deref_ty.strip_refs() else {
            // Type parameters (bounded generics) — can't determine module,
            // fall back to false; the emitter will check method_needs_export.
            return false;
        };
        let type_module = module_part(&id);
        let is_cross_module = type_module != self.cursor.module_id;

        if is_cross_module {
            return true;
        }

        let method_key = id.with_segment(member_name);
        store
            .get_definition(&method_key)
            .map(|d| d.visibility().is_public())
            .unwrap_or(false)
    }
}
