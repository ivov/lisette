use crate::checker::EnvResolve;
use ecow::EcoString;
use syntax::ast::{Expression, IdentifierResolution, Span, StructFields, StructKind};
use syntax::program::{Definition, DefinitionBody, DotAccessResolution, NativeTypeKind};
use syntax::types::{Symbol, Type, substitute};

use super::calls::phantom_type_params;
use crate::checker::infer::InferCtx;
use crate::checker::promotion::{self, MemberKind, Resolution};

pub(super) struct DotAccessResolutionArgs<'a> {
    pub(super) expression: &'a Expression,
    pub(super) expression_ty: &'a Type,
    /// `expression_ty.strip_refs()`, precomputed once per dot-access
    /// resolution so each `as_*` resolver doesn't recompute it.
    pub(super) deref_ty: Type,
    pub(super) member_name: &'a str,
    pub(super) span: &'a Span,
    pub(super) expected_ty: &'a Type,
}

impl DotAccessResolutionArgs<'_> {
    pub(super) fn build_dot_access(&self, ty: Type, resolution: DotAccessResolution) -> Expression {
        Expression::DotAccess {
            expression: self.expression.clone().into(),
            member: self.member_name.into(),
            ty,
            span: *self.span,
            resolution,
        }
    }
}

impl InferCtx<'_> {
    pub(super) fn normalize_aliased_ref(&self, ty: Type) -> Type {
        if ty.is_ref() {
            return ty;
        }
        let peeled = self.store.peel_alias(&ty);
        if peeled.is_ref() { peeled } else { ty }
    }

    pub(super) fn infer_dot_access(
        &mut self,
        expression: Box<Expression>,
        member: EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let expression_ty = self.new_type_var();
        let new_expression =
            self.with_dot_access_base(|state| state.infer_expression(*expression, &expression_ty));
        let resolved_expression_ty =
            self.normalize_aliased_ref(expression_ty.resolve_in(&self.env));

        let deref_ty = resolved_expression_ty.strip_refs();

        let args = DotAccessResolutionArgs {
            expression: &new_expression,
            expression_ty: &resolved_expression_ty,
            deref_ty,
            member_name: &member,
            span: &span,
            expected_ty,
        };

        if resolved_expression_ty.is_error() {
            self.unify(expected_ty, &Type::Error, &span);
            return args.build_dot_access(Type::Error, DotAccessResolution::Unresolved);
        }

        if resolved_expression_ty.is_variable() {
            self.sink
                .push(diagnostics::infer::unresolved_receiver_type(&member, span));
            self.unify(expected_ty, &Type::Error, &span);
            return args.build_dot_access(Type::Error, DotAccessResolution::Unresolved);
        }

        let resolved = self
            .as_struct_field(&args)
            .or_else(|| self.as_promoted_field(&args))
            .or_else(|| self.as_tuple_element(&args))
            .or_else(|| self.as_module_member(&args))
            .or_else(|| self.as_enum_variant(&args))
            .or_else(|| self.as_instance_method(&args))
            .or_else(|| self.as_static_method(&args));

        if let Some(expression) = resolved {
            if matches!(member.as_str(), "append" | "reserve")
                && resolved_expression_ty.is_ref()
                && args.deref_ty.has_name("Slice")
            {
                self.sink
                    .push(diagnostics::infer::ref_slice_growth(&member, span));
            }
            if member.as_str() == "equals" && self.scopes.is_callee_context() {
                self.gate_container_equals(&args.deref_ty, args.expression.get_span());
            }
            if !self.scopes.is_callee_context()
                && matches!(
                    expression.get_type().resolve_in(&self.env),
                    Type::Function(_) | Type::Forall { .. }
                )
                && NativeTypeKind::from_type(&resolved_expression_ty).is_some()
            {
                self.sink.push(diagnostics::infer::native_method_value(
                    &member,
                    diagnostics::infer::NativeMethodForm::Instance,
                    span,
                ));
            }
            return expression;
        }

        if promotion::has_direct_embed(self.store, &args.deref_ty)
            && let Resolution::Ambiguous { sources } =
                promotion::resolve_selector(self.store, &args.deref_ty, &member)
        {
            let names: Vec<String> = sources
                .iter()
                .map(|s| s.last_segment().to_string())
                .collect();
            self.sink.push(diagnostics::infer::ambiguous_selector(
                &args.deref_ty,
                &member,
                &names,
                span,
            ));
            self.unify(expected_ty, &Type::Error, &span);
            return args.build_dot_access(Type::Error, DotAccessResolution::Unresolved);
        }

        let display_ty = self.store.peel_alias(&resolved_expression_ty);
        let available_members = self.get_available_member_names(&resolved_expression_ty);
        let unwrap_hint = self.compute_unwrap_hint(&display_ty, &member);
        self.sink.push(diagnostics::infer::member_not_found(
            &display_ty,
            &member,
            span,
            if available_members.is_empty() {
                None
            } else {
                Some(&available_members)
            },
            unwrap_hint,
            self.scopes.is_callee_context(),
        ));

        args.build_dot_access(Type::Error, DotAccessResolution::Unresolved)
    }

    /// Whether a type's owning module is foreign (not current, prelude, or Go stdlib).
    /// Used to gate cross-module visibility checks on methods.
    pub(super) fn is_foreign_type(&self, type_id: &str) -> bool {
        let store = self.store;
        let type_module = store.module_for_qualified_name(type_id).unwrap_or(type_id);
        type_module != self.cursor.module_id
            && type_module != "prelude"
            && !type_module.starts_with("go:")
    }

    pub(super) fn is_type_level_receiver(&self, expression: &Expression) -> bool {
        let store = self.store;
        match expression {
            Expression::Identifier {
                resolution: IdentifierResolution::Definition(qname),
                ..
            } => store
                .get_definition(qname)
                .is_some_and(Definition::is_type_definition),
            Expression::DotAccess {
                expression: inner,
                member,
                ..
            } => {
                let inner_ty = inner.get_type().shallow_resolve_in(&self.env);
                let Some(module_id) = inner_ty.as_import_namespace() else {
                    return false;
                };
                let qualified = Symbol::from_parts(module_id, member.as_str());
                store
                    .get_definition(&qualified)
                    .is_some_and(Definition::is_type_definition)
            }
            _ => false,
        }
    }

    pub(super) fn method_is_promoted(&self, deref_ty: &Type, member: &str) -> bool {
        let Type::Nominal { id, .. } = deref_ty.strip_refs() else {
            return false;
        };
        promotion::has_direct_embed(self.store, deref_ty)
            && !self
                .store
                .get_own_methods(id.as_str())
                .is_some_and(|methods| methods.contains_key(member))
    }

    fn get_available_member_names(&mut self, ty: &Type) -> Vec<String> {
        let store = self.store;
        let deref_ty = ty.strip_refs();
        let mut names = Vec::new();

        if let Type::Nominal { id, .. } = &deref_ty
            && let Some(fields) = store.fields_of(id)
        {
            names.extend(fields.iter().map(|f| f.name.to_string()));
        }

        let methods = self.get_all_methods(store, &deref_ty);
        names.extend(methods.keys().map(|k| k.to_string()));

        names
    }

    fn compute_unwrap_hint(
        &mut self,
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
        if self.has_member(&inner, member) {
            Some(diagnostics::infer::UnwrapHint {
                wrapper,
                inner_ty: inner,
            })
        } else {
            None
        }
    }

    fn has_member(&mut self, ty: &Type, member: &str) -> bool {
        let store = self.store;
        let deref_ty = ty.strip_refs();

        if let Type::Nominal { id, .. } = &deref_ty
            && let Some(fields) = store.fields_of(id)
            && fields.iter().any(|f| f.name == member)
        {
            return true;
        }

        self.get_all_methods(store, &deref_ty).contains_key(member)
    }

    fn as_struct_field(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let store = self.store;
        let Type::Nominal {
            id: qualified_name, ..
        } = &args.deref_ty
        else {
            return None;
        };
        let resolved_struct_ty = store.peel_alias(&args.deref_ty);
        let Type::Nominal {
            id: struct_name, ..
        } = &resolved_struct_ty
        else {
            return None;
        };
        let struct_name = struct_name.clone();

        let Some(Definition {
            ty: struct_type,
            body:
                DefinitionBody::Struct {
                    fields: struct_fields,
                    generics,
                    ..
                },
            ..
        }) = store.get_definition(&struct_name)
        else {
            return None;
        };

        let struct_kind = struct_fields.kind();
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

        let struct_module = store
            .module_for_qualified_name(&struct_name)
            .unwrap_or(&struct_name);
        let is_cross_module = struct_module != self.cursor.module_id;

        if is_cross_module && !field_is_pub {
            self.sink.push(diagnostics::infer::private_field_access(
                args.member_name,
                qualified_name,
                struct_module,
                *args.span,
            ));
        }

        let (struct_ty, map) = self.instantiate(&struct_type);
        let field_ty = substitute(&field_type, &map);

        self.unify(&args.deref_ty, &struct_ty, args.span);
        self.unify(args.expected_ty, &field_ty, args.span);

        let is_exported = field_is_pub || is_cross_module;
        let resolution = if struct_kind == StructKind::Tuple {
            DotAccessResolution::TupleStructField { is_newtype }
        } else {
            DotAccessResolution::StructField { is_exported }
        };

        Some(args.build_dot_access(field_ty, resolution))
    }

    fn as_promoted_field(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let store = self.store;
        let Type::Nominal {
            id: qualified_name, ..
        } = &args.deref_ty
        else {
            return None;
        };
        if !promotion::has_direct_embed(store, &args.deref_ty) {
            return None;
        }

        let Resolution::Found(member) =
            promotion::resolve_selector(store, &args.deref_ty, args.member_name)
        else {
            return None;
        };
        let MemberKind::Field {
            ty: field_ty,
            visibility,
        } = member.kind
        else {
            return None;
        };

        if let Some(field) = store
            .fields_of(member.declaring_type.as_str())
            .and_then(|fields| fields.iter().find(|f| f.name == args.member_name))
        {
            self.facts.add_usage(*args.span, field.name_span);
        }

        let declaring_module = store
            .module_for_qualified_name(member.declaring_type.as_str())
            .unwrap_or_else(|| member.declaring_type.as_str());
        let is_cross_module = declaring_module != self.cursor.module_id;
        if is_cross_module && !visibility.is_public() {
            self.sink.push(diagnostics::infer::private_field_access(
                args.member_name,
                qualified_name.as_str(),
                declaring_module,
                *args.span,
            ));
        }

        self.unify(args.expected_ty, &field_ty, args.span);

        Some(args.build_dot_access(
            field_ty,
            DotAccessResolution::StructField {
                is_exported: visibility.is_public() || is_cross_module,
            },
        ))
    }

    fn as_tuple_element(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let index: usize = args.member_name.parse().ok()?;

        let peeled = self.store.peel_alias(&args.deref_ty);
        let Type::Tuple(elements) = &peeled else {
            return None;
        };

        if index >= elements.len() {
            return None;
        }

        let element_ty = elements[index].clone();
        self.unify(args.expected_ty, &element_ty, args.span);

        Some(args.build_dot_access(element_ty, DotAccessResolution::TupleElement))
    }

    fn as_module_member(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let store = self.store;
        let type_name = args.deref_ty.get_name()?;
        let namespace_id = args.deref_ty.as_import_namespace();

        // Look up by type-derived name first (works for non-aliased imports).
        // For aliased imports (e.g. `import u "utils"`), the map key is "u" but
        // the type name is "utils", so fall back to matching by import module id.
        let module_id = self
            .imports
            .namespace(type_name)
            .filter(|module_id| namespace_id.is_none_or(|namespace_id| *module_id == namespace_id))
            .or_else(|| {
                let module_id = namespace_id?;
                self.imports
                    .namespaces()
                    .find(|imported_module_id| *imported_module_id == module_id)
            })?;
        let module_id = module_id.to_string();
        let display_module = crate::loader::import_display_name(type_name);
        let module_ty = Type::ImportNamespace(module_id.clone().into());

        let resolved_definition = Symbol::from_parts(&module_id, args.member_name);
        let Some(definition) = store
            .get_module(&module_id)
            .and_then(|module| module.definitions.get(resolved_definition.as_str()))
            .filter(|definition| {
                definition.visibility.is_public() && !store.is_test_definition(definition)
            })
        else {
            self.sink
                .push(diagnostics::infer::function_or_value_not_found_in_module(
                    args.member_name,
                    display_module,
                    *args.span,
                ));
            return Some(args.build_dot_access(
                Type::Error,
                DotAccessResolution::ModuleMember { definition: None },
            ));
        };
        let member_type = self.resolve_definition_value_type(store, definition);

        if let Some(definition_span) = self.get_definition_name_span(store, &resolved_definition) {
            self.facts.add_usage(*args.span, definition_span);
        }

        self.check_module_member_in_value_position(
            store,
            &resolved_definition,
            &member_type,
            display_module,
            args,
        );

        let (module_ty, _) = self.instantiate(&module_ty);
        let (member_ty, _) = self.instantiate(&member_type);

        let coerced_to_unconstrained_value = !self.scopes.is_callee_context()
            && !self.scopes.is_dot_access_base()
            && args.expected_ty.resolve_in(&self.env).is_variable();

        self.unify(&args.deref_ty, &module_ty, args.span);
        self.unify(args.expected_ty, &member_ty, args.span);

        if coerced_to_unconstrained_value {
            let display_name = format!("{}.{}", display_module, args.member_name);
            self.register_function_value_obligations(&display_name, &member_ty, *args.span);
        }

        Some(args.build_dot_access(
            member_ty,
            DotAccessResolution::ModuleMember {
                definition: Some(resolved_definition),
            },
        ))
    }

    /// Rejects module members used in value position rather than called or used as a type.
    fn check_module_member_in_value_position(
        &mut self,
        store: &crate::store::Store,
        resolved_definition: &Symbol,
        member_type: &Type,
        display_module: &str,
        args: &DotAccessResolutionArgs,
    ) {
        let is_callee_context = self.scopes.is_callee_context();
        let is_dot_access_base = self.scopes.is_dot_access_base();
        let display_name = format!("{}.{}", display_module, args.member_name);

        if let Some(definition) = store.get_definition(resolved_definition) {
            match &definition.body {
                DefinitionBody::Struct {
                    fields: StructFields::Tuple(_),
                    ..
                } => {
                    // Deliberately omits `is_dot_access_base`, unlike the other arms below.
                    if !is_callee_context {
                        self.sink.push(diagnostics::infer::native_constructor_value(
                            &display_name,
                            *args.span,
                        ));
                    }
                }
                DefinitionBody::Struct {
                    fields: StructFields::Record(_),
                    ..
                } => {
                    if !is_callee_context && !is_dot_access_base {
                        self.sink.push(diagnostics::infer::record_struct_value(
                            &display_name,
                            *args.span,
                        ));
                    }
                }
                DefinitionBody::Enum { .. } => {
                    if !is_callee_context
                        && !is_dot_access_base
                        && !self.scopes.is_let_binding_rhs()
                    {
                        self.sink
                            .push(diagnostics::infer::namespace_alias_used_as_value(
                                *args.span,
                            ));
                    }
                }
                DefinitionBody::TypeAlias { .. } => {
                    if !is_callee_context && !is_dot_access_base {
                        let diagnostic = match store.deep_struct_kind(definition.ty.unwrap_forall())
                        {
                            Some(StructKind::Record) => {
                                diagnostics::infer::record_struct_value(&display_name, *args.span)
                            }
                            Some(StructKind::Tuple) => {
                                diagnostics::infer::native_constructor_value(
                                    &display_name,
                                    *args.span,
                                )
                            }
                            None => {
                                diagnostics::infer::type_used_as_value(&display_name, *args.span)
                            }
                        };
                        self.sink.push(diagnostic);
                    }
                }
                DefinitionBody::Interface { .. } if !is_callee_context && !is_dot_access_base => {
                    self.sink.push(diagnostics::infer::type_used_as_value(
                        &display_name,
                        *args.span,
                    ));
                }
                _ => {}
            }
        }

        if !is_callee_context && !is_dot_access_base {
            let phantom = phantom_type_params(member_type);
            if !phantom.is_empty() {
                self.sink
                    .push(diagnostics::infer::uninferable_generic_reference(
                        &display_name,
                        &phantom,
                        *args.span,
                    ));
            }
        }
    }

    fn as_enum_variant(&mut self, args: &DotAccessResolutionArgs) -> Option<Expression> {
        let store = self.store;
        let receiver_ty = match &args.deref_ty {
            Type::Nominal { .. } => args.deref_ty.clone(),
            Type::Function(f) => f.return_type.as_ref().clone(),
            _ => return None,
        };
        let Type::Nominal { id, .. } = store.peel_alias(&receiver_ty) else {
            return None;
        };

        let definition = store.get_definition(&id)?;

        let is_enum_variant = match &definition.body {
            DefinitionBody::Enum { variants, .. } => {
                variants.iter().any(|v| v.name == args.member_name)
            }
            _ => return None,
        };

        if !is_enum_variant {
            return None;
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
        self.unify(args.expected_ty, &variant_ty, args.span);
        Some(args.build_dot_access(
            variant_ty,
            DotAccessResolution::EnumVariant {
                definition: variant_qualified_name,
            },
        ))
    }
}
