use ecow::EcoString;
use syntax::ast::{Expression, Span, StructKind};
use syntax::program::{Definition, DotAccessKind, ReceiverCoercion};
use syntax::types::{Type, substitute};

use super::super::Checker;
use super::super::checks::check_is_non_addressable;
use super::primitives::contains_deref;

impl Checker<'_, '_> {
    pub(super) fn infer_dot_access_or_qualified_path(
        &mut self,
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
                    self.lookup_qualified_name(root).is_some()
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
                };
            }
        }

        if let Some(root) = expression.root_identifier()
            && let Some(qualified_root) = self.lookup_qualified_name(root)
            && let Some(base) = expression.as_dotted_path()
        {
            let path = format!("{}.{}", base, member);
            if self.lookup_type(&path).is_some() {
                self.track_name_usage(&qualified_root, &span, root.len() as u32);
                return self.infer_expression(
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

            let alias_target = self
                .store
                .get_definition(&qualified_root)
                .and_then(|definition| {
                    if let Definition::TypeAlias { ty: alias_ty, .. } = definition
                        && let Type::Constructor { id, params, .. } = alias_ty.unwrap_forall()
                        && params.is_empty()
                        && id.as_str() != qualified_root.as_str()
                    {
                        return Some(id.to_string());
                    }
                    None
                });

            if let Some(resolved_id) = alias_target {
                let mut paths = Vec::with_capacity(2);
                if let Some(short_name) = resolved_id.split('.').next_back()
                    && short_name != resolved_id
                {
                    paths.push(format!("{}.{}", short_name, member));
                }
                paths.push(format!("{}.{}", resolved_id, member));

                for path in paths {
                    if self.lookup_type(&path).is_some() {
                        return self.infer_expression(
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
            && let Some(qualified_root) = self.lookup_qualified_name(root)
            && let Some(Definition::TypeAlias { ty: alias_ty, .. }) =
                self.store.get_definition(&qualified_root)
        {
            let underlying = alias_ty.unwrap_forall();
            let is_generic = matches!(alias_ty, Type::Forall { .. })
                || matches!(underlying, Type::Constructor { params, .. } if !params.is_empty());
            if is_generic {
                let type_name = if let Type::Constructor { id, .. } = underlying {
                    id.split('.').next_back().unwrap_or(id).to_string()
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
                };
            }
        }

        self.infer_dot_access(expression, member, span, expected_ty)
    }
}

fn is_native_type(ty: &Type) -> bool {
    let resolved = ty.resolve().strip_refs();
    matches!(
        resolved.get_name(),
        Some("Slice" | "EnumeratedSlice" | "Map" | "Channel" | "Sender" | "Receiver" | "string")
    )
}

struct DotAccessResolutionArgs<'a> {
    expression: &'a Expression,
    expression_ty: &'a Type,
    member_name: &'a str,
    span: &'a Span,
    expected_ty: &'a Type,
}

impl Checker<'_, '_> {
    pub(super) fn infer_dot_access(
        &mut self,
        expression: Box<Expression>,
        member: EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let expression_ty = self.new_type_var();
        let new_expression = self.infer_expression(*expression, &expression_ty);
        let resolved_expression_ty = expression_ty.resolve();

        if resolved_expression_ty.is_error() {
            self.unify(expected_ty, &Type::Error, &span);
            return Expression::DotAccess {
                expression: new_expression.into(),
                member,
                ty: Type::Error,
                span,
            };
        }

        let args = DotAccessResolutionArgs {
            expression: &new_expression,
            expression_ty: &resolved_expression_ty,
            member_name: &member,
            span: &span,
            expected_ty,
        };

        let resolved = if let Some((expression, kind)) = self.as_struct_field(&args) {
            Some((expression, kind))
        } else if let Some(expression) = self.as_tuple_element(&args) {
            Some((expression, DotAccessKind::TupleElement))
        } else if let Some(expression) = self.as_module_member(&args) {
            Some((expression, DotAccessKind::ModuleMember))
        } else if let Some((expression, kind)) = self.as_enum_variant(&args) {
            Some((expression, kind))
        } else if let Some((expression, kind)) = self.as_instance_method(&args) {
            Some((expression, kind))
        } else {
            self.as_static_method(&args)
        };

        if let Some((expression, kind)) = resolved {
            self.resolutions.mark_dot_access(span, kind);

            if (member.as_str() == "append" || member.as_str() == "extend")
                && resolved_expression_ty.is_ref()
                && resolved_expression_ty.strip_refs().has_name("Slice")
            {
                self.sink.push(diagnostics::infer::ref_slice_append(span));
            }
            if !self.scopes.is_callee_context()
                && matches!(
                    expression.get_type().resolve(),
                    Type::Function { .. } | Type::Forall { .. }
                )
                && is_native_type(&resolved_expression_ty)
            {
                self.sink
                    .push(diagnostics::infer::native_method_value(&member, span));
            }
            return expression;
        }

        let available_members = self.get_available_member_names(&resolved_expression_ty);
        self.sink.push(diagnostics::infer::member_not_found(
            &resolved_expression_ty,
            &member,
            span,
            if available_members.is_empty() {
                None
            } else {
                Some(&available_members)
            },
        ));

        Expression::DotAccess {
            expression: new_expression.into(),
            member,
            ty: Type::Error,
            span,
        }
    }

    /// Whether a type's owning module is foreign (not current, prelude, or Go stdlib).
    /// Used to gate cross-module visibility checks on methods.
    fn is_foreign_type(&self, type_id: &str) -> bool {
        let type_module = type_id.split('.').next().unwrap_or(type_id);
        type_module != self.cursor.module_id
            && type_module != "prelude"
            && !type_module.starts_with("go:")
    }

    fn get_available_member_names(&self, ty: &Type) -> Vec<String> {
        let deref_ty = ty.strip_refs();
        let mut names = Vec::new();

        if let Type::Constructor { .. } = deref_ty {
            let qualified_name = deref_ty.get_qualified_name();
            if let Some(fields) = self.store.get_struct_fields(&qualified_name) {
                names.extend(fields.iter().map(|f| f.name.to_string()));
            }
        }

        let methods = self.get_all_methods(&deref_ty);
        names.extend(methods.into_keys().map(|k| k.to_string()));

        names
    }

    fn as_struct_field(
        &mut self,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let deref_ty = args.expression_ty.strip_refs();

        let Type::Constructor { .. } = deref_ty else {
            return None;
        };

        let qualified_name = deref_ty.get_qualified_name();

        let Some(Definition::Struct {
            ty: struct_type,
            fields: struct_fields,
            kind: struct_kind,
            generics,
            ..
        }) = self.store.get_definition(&qualified_name)
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

        let struct_module = qualified_name.split('.').next().unwrap_or(&qualified_name);
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

        self.unify(&deref_ty, &struct_ty, args.span);
        self.unify(args.expected_ty, &field_ty, args.span);

        let is_exported = field_is_pub || is_cross_module;
        let kind = if struct_kind == StructKind::Tuple {
            DotAccessKind::TupleStructField { is_newtype }
        } else {
            DotAccessKind::StructField { is_exported }
        };

        Some((
            Expression::DotAccess {
                expression: args.expression.clone().into(),
                member: args.member_name.into(),
                ty: field_ty,
                span: *args.span,
            },
            kind,
        ))
    }

    fn as_tuple_element(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let index: usize = args.member_name.parse().ok()?;

        let deref_ty = args.expression_ty.strip_refs();

        let Type::Tuple(elements) = &deref_ty else {
            return None;
        };

        if index >= elements.len() {
            return None;
        }

        let element_ty = elements[index].clone();
        self.unify(args.expected_ty, &element_ty, args.span);

        Some(Expression::DotAccess {
            expression: args.expression.clone().into(),
            member: args.member_name.into(),
            ty: element_ty,
            span: *args.span,
        })
    }

    fn as_module_member(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let deref_ty = args.expression_ty.strip_refs();
        let type_name = deref_ty.get_name()?;

        // Look up by type-derived name first (works for non-aliased imports).
        // For aliased imports (e.g. `import u "utils"`), the map key is "u" but
        // the type name is "utils", so fall back to matching by type ID.
        let (module_fields, module_ty) = self
            .imports
            .imported_modules
            .get(type_name)
            .cloned()
            .or_else(|| {
                if let Type::Constructor { id, .. } = &deref_ty {
                    self.imports
                        .imported_modules
                        .values()
                        .find(|(_, ty)| match ty {
                            Type::Constructor { id: ty_id, .. } => ty_id == id,
                            _ => false,
                        })
                        .cloned()
                } else {
                    None
                }
            })?;

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
            return Some(Expression::DotAccess {
                expression: args.expression.clone().into(),
                member: args.member_name.into(),
                ty: Type::Error,
                span: *args.span,
            });
        };

        // module_ty.id is "@import/module_id", extract actual module_id
        if let Type::Constructor {
            id: module_type_id, ..
        } = &module_ty
        {
            let module_id = module_type_id
                .strip_prefix("@import/")
                .unwrap_or(module_type_id);
            let qualified_name = format!("{}.{}", module_id, args.member_name);
            if let Some(definition_span) = self.get_definition_name_span(&qualified_name) {
                self.facts.add_usage(*args.span, definition_span);
            }

            // Reject cross-module tuple-struct constructors used as values
            if !self.scopes.is_callee_context()
                && matches!(
                    self.store.get_definition(&qualified_name),
                    Some(Definition::Struct {
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
        }

        let (module_ty, _) = self.instantiate(&module_ty);
        let (member_ty, _) = self.instantiate(&member_type);

        self.unify(&deref_ty, &module_ty, args.span);
        self.unify(args.expected_ty, &member_ty, args.span);

        Some(Expression::DotAccess {
            expression: args.expression.clone().into(),
            member: args.member_name.into(),
            ty: member_ty,
            span: *args.span,
        })
    }

    fn as_instance_method(
        &mut self,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let deref_ty = args.expression_ty.strip_refs();

        if !matches!(deref_ty, Type::Constructor { .. } | Type::Parameter(_)) {
            return None;
        }

        let method_ty = self
            .get_all_methods(&deref_ty)
            .get(args.member_name)
            .cloned()?;

        self.check_instance_method_access(&deref_ty, &method_ty, args);

        let is_exported = self.is_dot_access_exported(&deref_ty, args.member_name);
        let kind = DotAccessKind::InstanceMethod { is_exported };

        let (mut method_ty, _) = self.instantiate(&method_ty);

        if !matches!(method_ty, Type::Function { .. }) {
            return None;
        }

        if let Some(expression) = self.as_method_value(args, &mut method_ty) {
            let is_pointer_receiver = if let Type::Function { params, .. } = &method_ty {
                !params.is_empty() && params[0].resolve().is_ref()
            } else {
                false
            };
            let value_kind = DotAccessKind::InstanceMethodValue {
                is_exported,
                is_pointer_receiver,
            };
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

        self.unify_receiver_with_coercion(
            &receiver_ty,
            actual_ty,
            args.expression,
            args.member_name,
            args.span,
        );

        self.unify(args.expected_ty, &method_ty, args.span);

        Some((
            Expression::DotAccess {
                expression: args.expression.clone().into(),
                member: args.member_name.into(),
                ty: method_ty,
                span: *args.span,
            },
            kind,
        ))
    }

    /// Check cross-module visibility, record usage for find-references,
    /// and warn if a UFCS method is taken as a value.
    fn check_instance_method_access(
        &mut self,
        deref_ty: &Type,
        method_ty: &Type,
        args: &DotAccessResolutionArgs,
    ) {
        if let Type::Constructor { .. } = deref_ty {
            let qualified_name = deref_ty.get_qualified_name();
            let method_key = format!("{}.{}", qualified_name, args.member_name);

            if let Some(definition_span) = self.get_definition_name_span(&method_key) {
                self.facts.add_usage(*args.span, definition_span);
            }

            if self.is_foreign_type(&qualified_name)
                && let Some(Definition::Value { visibility, .. }) =
                    self.store.get_definition(&method_key)
                && !visibility.is_public()
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
            && vars.len() > self.get_receiver_generics_count(deref_ty)
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
        args: &DotAccessResolutionArgs,
        method_ty: &mut Type,
    ) -> Option<Expression> {
        let Type::Function { params, .. } = &*method_ty else {
            return None;
        };

        let is_cross_module_type_access = matches!(
            args.expression,
            Expression::DotAccess { expression: inner, .. }
                if matches!(inner.get_type().resolve(),
                    Type::Constructor { ref id, .. } if id.starts_with("@import/"))
        );

        if !is_cross_module_type_access || self.scopes.is_callee_context() {
            return None;
        }

        // Don't remove self — the value type should include the receiver.
        // Still unify the receiver type with the expression type for generic resolution.
        let receiver_ty = params[0].resolve();
        let receiver_stripped = receiver_ty.strip_refs();
        let expression_stripped = args.expression_ty.resolve().strip_refs();
        self.unify(&receiver_stripped, &expression_stripped, args.span);

        self.unify(args.expected_ty, method_ty, args.span);

        Some(Expression::DotAccess {
            expression: args.expression.clone().into(),
            member: args.member_name.into(),
            ty: method_ty.clone(),
            span: *args.span,
        })
    }

    /// Unifies receiver type with coercion support for method calls.
    /// Matches Go's behavior: auto-address (T → Ref<T>) and auto-deref (Ref<T> → T).
    fn unify_receiver_with_coercion(
        &mut self,
        receiver_ty: &Type,
        actual_ty: &Type,
        receiver_expression: &Expression,
        method_name: &str,
        span: &Span,
    ) {
        // Resolve to follow any type variable links before checking is_ref
        let receiver_ty = receiver_ty.resolve();
        let actual_ty = actual_ty.resolve();
        let receiver_is_ref = receiver_ty.is_ref();
        let actual_is_ref = actual_ty.is_ref();

        match (receiver_is_ref, actual_is_ref) {
            (true, false) => {
                // Method expects Ref<T>, have T → auto-address
                if let Some(kind) = check_is_non_addressable(receiver_expression) {
                    self.sink
                        .push(diagnostics::infer::cannot_auto_address_receiver(
                            kind,
                            method_name,
                            &receiver_ty,
                            &actual_ty,
                            *span,
                        ));
                } else {
                    self.coercions.mark_coercion(
                        receiver_expression.get_span(),
                        ReceiverCoercion::AutoAddress,
                    );
                    self.check_auto_address_mutation(receiver_expression, method_name, span);
                }
                // Unify inner types: T with T (from Ref<T>)
                if let Some(inner) = receiver_ty.inner() {
                    self.unify(&inner, &actual_ty, span);
                }
            }
            (false, true) => {
                // Method expects T, have Ref<T> → auto-deref
                self.coercions
                    .mark_coercion(receiver_expression.get_span(), ReceiverCoercion::AutoDeref);
                // Unify inner types: T with T (from Ref<T>)
                if let Some(inner) = actual_ty.inner() {
                    self.unify(&receiver_ty, &inner, span);
                }
            }
            (true, true) => {
                // Both are refs — normal unification (handles same depth)
                // Note: Multi-level mismatches (Ref<Ref<T>> vs Ref<T>) will fail in unify
                self.unify(&receiver_ty, &actual_ty, span);
            }
            (false, false) => {
                // Neither is ref — normal unification
                self.unify(&receiver_ty, &actual_ty, span);
            }
        }
    }

    /// When auto-addressing a receiver (T → Ref<T>), verify the binding
    /// is declared `let mut`, since the Ref<T> method may mutate it.
    fn check_auto_address_mutation(
        &mut self,
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
            .map(|t| t.resolve().is_ref())
            .unwrap_or(false);
        if !is_deref && !binding_is_ref && !self.scopes.lookup_mutable(&var_name) {
            let self_type_name = if var_name == "self" {
                self.lookup_type("self")
                    .and_then(|t| t.get_name().map(str::to_owned))
            } else {
                None
            };
            self.sink.push(diagnostics::infer::disallowed_mutation(
                &var_name,
                *span,
                self_type_name.as_deref(),
            ));
        }
    }

    pub(crate) fn get_receiver_generics_count(&self, receiver_ty: &Type) -> usize {
        let Type::Constructor { id, .. } = receiver_ty else {
            return 0;
        };

        match self.store.get_definition(id) {
            Some(Definition::Struct { generics, .. }) => generics.len(),
            Some(Definition::TypeAlias { generics, .. }) => generics.len(),
            Some(Definition::Enum { generics, .. }) => generics.len(),
            _ => 0,
        }
    }

    fn as_enum_variant(
        &mut self,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let deref_ty = args.expression_ty.strip_refs();

        let id = match deref_ty {
            Type::Constructor { id, .. } => id.clone(),
            Type::Function { return_type, .. } => {
                if let Type::Constructor { id, .. } = return_type.as_ref() {
                    id.clone()
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        let definition = self.store.get_definition(&id)?;

        let (is_enum_variant, kind) = match definition {
            Definition::Enum { variants, .. } => (
                variants.iter().any(|v| v.name == args.member_name),
                DotAccessKind::EnumVariant,
            ),
            Definition::ValueEnum { variants, .. } => (
                variants.iter().any(|v| v.name == args.member_name),
                DotAccessKind::ValueEnumVariant,
            ),
            _ => return None,
        };

        if !is_enum_variant {
            return None;
        }

        if let Definition::ValueEnum { methods, .. } = definition
            && methods.contains_key(args.member_name)
        {
            let is_type_access = matches!(
                args.expression,
                Expression::DotAccess { expression, .. }
                    if matches!(expression.get_type().resolve(),
                        Type::Constructor { id, .. } if id.starts_with("@import/"))
            );
            if !is_type_access {
                return None;
            }
        }

        let variant_qualified_name = format!("{}.{}", id, args.member_name);
        let variant_definition = self.store.get_definition(&variant_qualified_name)?;

        let Definition::Value {
            ty: variant_ty,
            visibility,
            name_span,
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
        self.unify(args.expected_ty, &variant_ty, args.span);

        Some((
            Expression::DotAccess {
                expression: args.expression.clone().into(),
                member: args.member_name.into(),
                ty: variant_ty,
                span: *args.span,
            },
            kind,
        ))
    }

    fn as_static_method(
        &mut self,
        args: &DotAccessResolutionArgs,
    ) -> Option<(Expression, DotAccessKind)> {
        let deref_ty = args.expression_ty.strip_refs();

        let id = match deref_ty {
            Type::Function {
                ref return_type, ..
            } => {
                if let Type::Constructor { id, .. } = return_type.as_ref() {
                    id.clone()
                } else {
                    return None;
                }
            }
            Type::Constructor { ref id, .. } => {
                // For enums with Constructor type, we need to distinguish between:
                // - Type access (e.g., `module.Color.default()`) - ALLOW
                // - Value access (e.g., `c.new()` where c is a Color value) - REJECT
                //
                // Type access comes through DotAccess on a module import.
                // Value access comes through an Identifier or other expression.
                if let Some(Definition::Enum { .. } | Definition::ValueEnum { .. }) =
                    self.store.get_definition(id)
                {
                    // Check if expression is a module member access (type-level access)
                    let is_type_access = matches!(
                        args.expression,
                        Expression::DotAccess { expression, .. }
                            if matches!(expression.get_type().resolve(),
                                Type::Constructor { id, .. } if id.starts_with("@import/"))
                    );
                    if !is_type_access {
                        return None;
                    }
                }
                id.clone()
            }
            _ => return None,
        };

        if self
            .get_all_methods(&deref_ty)
            .contains_key(args.member_name)
        {
            return None;
        }

        let method_qualified_name = format!("{}.{}", id, args.member_name);
        let method_definition = self.store.get_definition(&method_qualified_name)?;

        let Definition::Value {
            ty: method_ty,
            name_span,
            visibility,
            ..
        } = method_definition
        else {
            return None;
        };

        let method_ty = method_ty.clone();
        let name_span = *name_span;
        let is_public = visibility.is_public();

        if self.is_foreign_type(&id) && !is_public {
            let type_simple_name = id.rsplit('.').next().unwrap_or(&id);
            self.sink.push(diagnostics::infer::private_method_access(
                args.member_name,
                type_simple_name,
                *args.span,
            ));
        }

        if let Some(definition_span) = name_span {
            self.facts.add_usage(*args.span, definition_span);
        }

        let type_name_len = id.rsplit('.').next().unwrap_or(&id).len() as u32;
        self.track_name_usage(&id, args.span, type_name_len);

        let (method_ty, _) = self.instantiate(&method_ty);

        self.unify(args.expected_ty, &method_ty, args.span);

        let type_module = id.split('.').next().unwrap_or("");
        let is_cross_module = type_module != self.cursor.module_id;
        let is_exported = is_public || is_cross_module;

        Some((
            Expression::DotAccess {
                expression: args.expression.clone().into(),
                member: args.member_name.into(),
                ty: method_ty,
                span: *args.span,
            },
            DotAccessKind::StaticMethod { is_exported },
        ))
    }

    fn is_dot_access_exported(&self, deref_ty: &Type, member_name: &str) -> bool {
        let Type::Constructor { id, .. } = deref_ty.strip_refs() else {
            // Type parameters (bounded generics) — can't determine module,
            // fall back to false; the emitter will check method_needs_export.
            return false;
        };
        let type_module = id.split('.').next().unwrap_or("");
        let is_cross_module = type_module != self.cursor.module_id;

        if is_cross_module {
            return true;
        }

        let method_key = format!("{}.{}", id, member_name);
        self.store
            .get_definition(&method_key)
            .map(|d| d.visibility().is_public())
            .unwrap_or(false)
    }
}
