use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;
use syntax::ast::BindingKind;
use syntax::ast::{
    EnumFieldDefinition, Expression, Literal, Pattern, RestPattern, Span, StructFieldPattern,
    TypedPattern, collect_pattern_bindings,
};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Type, substitute, unqualified_name};

use crate::checker::EnvResolve;

use crate::checker::infer::InferCtx;

impl InferCtx<'_, '_> {
    pub(super) fn infer_pattern(
        &mut self,
        pattern: Pattern,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        self.infer_pattern_inner(pattern, expected_ty, kind, false)
    }

    fn infer_pattern_inner(
        &mut self,
        pattern: Pattern,
        expected_ty: Type,
        kind: BindingKind,
        is_struct_field: bool,
    ) -> (Pattern, TypedPattern) {
        let store = self.store;
        match pattern {
            Pattern::Identifier { identifier, span } => {
                let is_d_lis = self.is_d_lis(store);
                self.bind_name_in_scope(
                    identifier.to_string(),
                    span,
                    expected_ty,
                    kind,
                    is_d_lis,
                    is_struct_field,
                    false,
                );
                (
                    Pattern::Identifier { identifier, span },
                    TypedPattern::Wildcard,
                )
            }

            Pattern::Literal { literal, ty, span } => {
                let inferred_literal =
                    self.infer_expression(Expression::Literal { literal, ty, span }, &expected_ty);

                match inferred_literal {
                    Expression::Literal { literal, ty, span } => {
                        let typed = TypedPattern::Literal(literal.clone());
                        (Pattern::Literal { literal, ty, span }, typed)
                    }
                    _ => unreachable!(),
                }
            }

            Pattern::Tuple { elements, span } => {
                let element_types: Vec<Type> = match &expected_ty {
                    Type::Tuple(types) if types.len() == elements.len() => types.clone(),
                    Type::Tuple(types) => {
                        self.sink.push(diagnostics::infer::tuple_arity_mismatch(
                            elements.len(),
                            types.len(),
                            span,
                        ));
                        elements.iter().map(|_| Type::Error).collect()
                    }
                    _ => {
                        let vars: Vec<Type> =
                            elements.iter().map(|_| self.new_type_var()).collect();
                        let tuple_ty = Type::Tuple(vars.clone());
                        self.unify(&expected_ty, &tuple_ty, &span);
                        vars
                    }
                };

                let (inferred_elements, typed_elements): (Vec<_>, Vec<_>) = elements
                    .into_iter()
                    .zip(element_types.iter())
                    .map(|(p, ty)| self.infer_pattern_inner(p, ty.clone(), kind, false))
                    .unzip();

                let pattern = Pattern::Tuple {
                    elements: inferred_elements,
                    span,
                };
                let typed = TypedPattern::Tuple {
                    arity: typed_elements.len(),
                    elements: typed_elements,
                };
                (pattern, typed)
            }

            Pattern::EnumVariant {
                identifier,
                fields,
                rest,
                span,
                ..
            } => self.infer_enum_variant_pattern(identifier, fields, rest, span, expected_ty, kind),

            Pattern::Struct {
                identifier,
                fields,
                rest,
                span,
                ..
            } => self.infer_struct_pattern(identifier, fields, rest, span, expected_ty, kind),

            Pattern::WildCard { span } => (Pattern::WildCard { span }, TypedPattern::Wildcard),

            Pattern::Unit { span, .. } => {
                let unit_ty = self.type_unit();
                self.unify(&expected_ty, &unit_ty, &span);
                (Pattern::Unit { ty: unit_ty, span }, TypedPattern::Wildcard)
            }

            Pattern::Slice {
                prefix, rest, span, ..
            } => {
                let resolved_ty = expected_ty.resolve_in(&self.env);
                let element_ty = match resolved_ty.as_compound() {
                    Some((syntax::types::CompoundKind::Slice, args)) if args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => {
                        let element_ty = self.new_type_var();
                        let slice_ty = self.type_slice(element_ty.clone());
                        self.unify(&expected_ty, &slice_ty, &span);
                        element_ty
                    }
                };

                let (inferred_prefix, typed_prefix): (Vec<_>, Vec<_>) = prefix
                    .into_iter()
                    .map(|p| self.infer_pattern_inner(p, element_ty.clone(), kind, false))
                    .unzip();

                if let RestPattern::Bind { ref name, ref span } = rest {
                    let rest_ty = if element_ty.shallow_resolve_in(&self.env).is_error() {
                        Type::Error
                    } else {
                        self.type_slice(element_ty.clone())
                    };
                    let is_typedef = self.is_d_lis(store);
                    let binding_id = self.facts.add_binding(
                        name.to_string(),
                        *span,
                        kind,
                        is_typedef,
                        false,
                        false,
                    );
                    let scope = self.scopes.current_mut();
                    scope.values.insert(name.to_string(), rest_ty);
                    scope.name_to_binding.insert(name.to_string(), binding_id);
                }

                let pattern = Pattern::Slice {
                    prefix: inferred_prefix,
                    rest: rest.clone(),
                    element_ty: element_ty.clone(),
                    span,
                };
                let typed = TypedPattern::Slice {
                    prefix: typed_prefix,
                    has_rest: rest.is_present(),
                    element_type: element_ty,
                };
                (pattern, typed)
            }

            Pattern::Or { patterns, span } => {
                self.infer_or_pattern(patterns, span, expected_ty, kind)
            }

            Pattern::AsBinding {
                pattern,
                name,
                span,
            } => {
                if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    self.sink
                        .push(diagnostics::infer::uppercase_binding(span, &name));
                }
                match pattern.as_ref() {
                    Pattern::Identifier { identifier, .. } => {
                        self.sink.push(diagnostics::infer::redundant_as_identifier(
                            identifier, &name, span,
                        ));
                    }
                    Pattern::WildCard { .. } => {
                        self.sink
                            .push(diagnostics::infer::redundant_as_wildcard(&name, span));
                    }
                    Pattern::Literal { literal, .. } => {
                        self.sink.push(diagnostics::infer::redundant_as_literal(
                            &format_literal(literal),
                            &name,
                            span,
                        ));
                    }
                    _ => {}
                }
                let inner_kind = match kind {
                    BindingKind::Let { .. } => BindingKind::Let { mutable: false },
                    BindingKind::Parameter { .. } => BindingKind::Parameter { mutable: false },
                    other => other,
                };
                let (inner, typed) = self.infer_pattern_inner(
                    *pattern,
                    expected_ty.clone(),
                    inner_kind,
                    is_struct_field,
                );
                let alias_ty = inner.get_type().unwrap_or_else(|| expected_ty.clone());
                let name_span = Span::new(
                    span.file_id,
                    span.byte_offset + span.byte_length - name.len() as u32,
                    name.len() as u32,
                );
                self.bind_name_in_scope(
                    name.to_string(),
                    name_span,
                    alias_ty,
                    kind,
                    false,
                    is_struct_field,
                    true,
                );
                (
                    Pattern::AsBinding {
                        pattern: Box::new(inner),
                        name,
                        span,
                    },
                    typed,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_name_in_scope(
        &mut self,
        name: String,
        span: Span,
        ty: Type,
        kind: BindingKind,
        is_typedef: bool,
        is_struct_field: bool,
        is_as_alias: bool,
    ) {
        let binding_id = self.facts.add_binding(
            name.clone(),
            span,
            kind,
            is_typedef,
            is_struct_field,
            is_as_alias,
        );
        let scope = self.scopes.current_mut();
        scope.values.insert(name.clone(), ty);
        scope.name_to_binding.insert(name.clone(), binding_id);
        if kind.is_mutable() {
            scope
                .mutables
                .get_or_insert_with(HashSet::default)
                .insert(name);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_enum_variant_pattern(
        &mut self,
        identifier: EcoString,
        fields: Vec<Pattern>,
        rest: bool,
        span: Span,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        let store = self.store;
        if fields.is_empty()
            && identifier.contains('.')
            && let Some(result) =
                self.try_infer_const_pattern(&identifier, rest, kind, &expected_ty, span)
        {
            return result;
        }

        let is_bare_name =
            fields.is_empty() && !identifier.contains('.') && !kind.is_pattern_position();

        let bare_variant_ty = if kind.is_match_arm() {
            self.resolve_bare_variant_type(&identifier, &expected_ty)
        } else {
            None
        };

        let ty = if let Some(ty) = bare_variant_ty {
            ty
        } else if let Some(ty) = self.lookup_type(store, &identifier) {
            ty
        } else if let Some((alias_ty, _)) = self.try_resolve_type_alias_variant(&identifier) {
            alias_ty
        } else {
            if is_bare_name {
                self.sink
                    .push(diagnostics::infer::uppercase_binding(span, &identifier));
                return (Pattern::WildCard { span }, TypedPattern::Wildcard);
            }
            let enum_info = self.get_enum_variant_info(&expected_ty);
            let bare_name = unqualified_name(&identifier);
            self.sink
                .push(diagnostics::infer::enum_variant_constructor_not_found(
                    span,
                    enum_info.as_ref().map(|(n, v)| (n.as_str(), v.as_slice())),
                    bare_name,
                    kind.is_match_arm(),
                ));
            return (Pattern::WildCard { span }, TypedPattern::Wildcard);
        };

        let (value_constructor_type, _) = self.instantiate(&ty);

        if is_bare_name
            && matches!(
                &value_constructor_type,
                Type::Nominal { .. } | Type::Compound { .. } | Type::Simple(_)
            )
            && !self.is_enum_type(store, &value_constructor_type)
        {
            self.sink
                .push(diagnostics::infer::uppercase_binding(span, &identifier));
            return (Pattern::WildCard { span }, TypedPattern::Wildcard);
        }

        let (pattern_ty, params) = match value_constructor_type {
            Type::Function(f) => {
                let f = std::sync::Arc::try_unwrap(f).unwrap_or_else(|arc| (*arc).clone());
                (*f.return_type, f.params)
            }
            Type::Nominal { .. } | Type::Compound { .. } | Type::Simple(_) => {
                (value_constructor_type, vec![])
            }
            _ => unreachable!(),
        };

        let unify_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        self.unify(&unify_expected, &pattern_ty, &span);

        let (new_fields, mut typed_fields): (Vec<_>, Vec<_>) = fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let param_ty = params.get(i).cloned().unwrap_or_else(|| Type::Error);
                self.infer_pattern_inner(f.clone(), param_ty, kind, false)
            })
            .unzip();

        if rest {
            for _ in new_fields.len()..params.len() {
                typed_fields.push(TypedPattern::Wildcard);
            }
        } else if params.len() != new_fields.len() {
            let actual_types: Vec<Type> = new_fields
                .iter()
                .map(|p| p.get_type().unwrap_or_else(|| self.new_type_var()))
                .collect();
            self.sink.push(diagnostics::infer::arity_mismatch(
                &params,
                &actual_types,
                &[],
                true,
                span,
            ));
        }

        let resolved_field_types: Box<[Type]> =
            params.iter().map(|p| p.resolve_in(&self.env)).collect();

        let resolved_ty = pattern_ty.resolve_in(&self.env);
        let typed = match &resolved_ty {
            Type::Nominal { id, params, .. } => {
                let variant_name = unqualified_name(&identifier);
                let variant_qualified = id.with_segment(variant_name);
                if let Some(definition_span) =
                    self.get_definition_name_span(store, &variant_qualified)
                {
                    self.facts.add_usage(span, definition_span);
                }

                let variant_fields = store
                    .variants_of(id)
                    .and_then(|variants| {
                        variants
                            .iter()
                            .find(|v| v.name == variant_name)
                            .map(|v| v.fields.iter().cloned().collect())
                    })
                    .unwrap_or_default();

                TypedPattern::EnumVariant {
                    enum_name: id.into(),
                    variant_name: identifier.clone(),
                    variant_fields,
                    fields: typed_fields,
                    type_args: params.clone(),
                    field_types: resolved_field_types,
                }
            }
            _ => TypedPattern::Wildcard,
        };

        let pattern = Pattern::EnumVariant {
            identifier,
            fields: new_fields,
            rest,
            ty: pattern_ty,
            span,
        };
        (pattern, typed)
    }

    fn try_infer_const_pattern(
        &mut self,
        identifier: &str,
        rest: bool,
        kind: BindingKind,
        expected_ty: &Type,
        span: Span,
    ) -> Option<(Pattern, TypedPattern)> {
        let store = self.store;
        let qualified = self.lookup_qualified_name(store, identifier)?;
        let definition = store.get_definition(&qualified)?;
        if !matches!(definition.body, DefinitionBody::Value { .. }) {
            return None;
        }

        let definition_ty = definition.ty();
        let unwrapped_ty = definition_ty.unwrap_forall();

        // Enum-variant resolution takes precedence, so a unit variant of its own
        // enum type stays on the enum-variant path.
        let member_name = unqualified_name(&qualified);
        if let Type::Nominal { id, .. } = unwrapped_ty
            && store
                .variants_of(id.as_str())
                .is_some_and(|variants| variants.iter().any(|v| v.name == member_name))
        {
            return None;
        }

        if !kind.is_match_arm() {
            self.sink
                .push(diagnostics::infer::const_pattern_outside_match_arm(
                    identifier, span,
                ));
            return Some((Pattern::WildCard { span }, TypedPattern::Wildcard));
        }

        if matches!(unwrapped_ty, Type::Function(_)) {
            self.sink
                .push(diagnostics::infer::const_pattern_not_eligible(
                    identifier, span,
                ));
            return Some((Pattern::WildCard { span }, TypedPattern::Wildcard));
        }

        let (const_ty, _) = self.instantiate(definition_ty);
        let const_value = definition.const_value().cloned();
        let unify_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        self.unify(&unify_expected, &const_ty, &span);

        if let Some(definition_span) = self.get_definition_name_span(store, &qualified) {
            self.facts.add_usage(span, definition_span);
        }

        let resolved_ty = const_ty.resolve_in(&self.env);
        let pattern = Pattern::EnumVariant {
            identifier: identifier.into(),
            fields: vec![],
            rest,
            ty: resolved_ty.clone(),
            span,
        };
        let typed = TypedPattern::Const {
            qualified_name: qualified,
            ty: resolved_ty,
            value: const_value,
        };
        Some((pattern, typed))
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_struct_pattern(
        &mut self,
        identifier: EcoString,
        fields: Vec<StructFieldPattern>,
        rest: bool,
        span: Span,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        let store = self.store;
        if kind.is_match_arm()
            && self
                .resolve_bare_variant_type(&identifier, &expected_ty)
                .is_some()
            && let Some(result) = self.try_infer_enum_struct_variant(
                &identifier,
                &fields,
                rest,
                &span,
                &expected_ty,
                kind,
            )
        {
            return result;
        }

        let Some(qualified_name) = self.lookup_qualified_name(store, &identifier) else {
            return self
                .try_infer_enum_struct_variant(
                    &identifier,
                    &fields,
                    rest,
                    &span,
                    &expected_ty,
                    kind,
                )
                .unwrap_or_else(|| {
                    self.sink
                        .push(diagnostics::infer::struct_not_found(&identifier, span));
                    (Pattern::WildCard { span }, TypedPattern::Wildcard)
                });
        };
        let Some(Definition {
            ty: struct_forall_ty,
            body:
                DefinitionBody::Struct {
                    fields: definition_struct_fields,
                    ..
                },
            ..
        }) = store.get_definition(&qualified_name)
        else {
            return self
                .try_infer_enum_struct_variant(
                    &identifier,
                    &fields,
                    rest,
                    &span,
                    &expected_ty,
                    kind,
                )
                .unwrap_or_else(|| {
                    self.sink
                        .push(diagnostics::infer::struct_not_found(&identifier, span));
                    (Pattern::WildCard { span }, TypedPattern::Wildcard)
                });
        };

        let struct_forall_ty = struct_forall_ty.clone();
        let struct_fields = definition_struct_fields.clone();

        self.track_name_usage(store, &qualified_name, &span, identifier.len() as u32);

        let (struct_ty, map) = self.instantiate(&struct_forall_ty);

        self.unify(&expected_ty, &struct_ty, &span);

        let scrutinee_is_error = expected_ty.shallow_resolve_in(&self.env).is_error();

        let struct_module = store
            .module_for_qualified_name(&qualified_name)
            .unwrap_or(&qualified_name);
        let is_cross_module = struct_module != self.cursor.module_id;

        let available: Vec<String> = struct_fields.iter().map(|f| f.name.to_string()).collect();

        let (new_fields, typed_field_values): (Vec<_>, Vec<_>) = fields
            .iter()
            .map(|field| {
                let field_definition = struct_fields.iter().find(|x| x.name == field.name);

                let field_ty = match field_definition {
                    Some(field_definition) => {
                        if is_cross_module && !field_definition.visibility.is_public() {
                            self.sink.push(diagnostics::infer::private_field_access(
                                &field.name,
                                &qualified_name,
                                field.value.get_span(),
                            ));
                        }
                        if scrutinee_is_error {
                            Type::Error
                        } else {
                            substitute(&field_definition.ty, &map)
                        }
                    }
                    None => {
                        self.sink.push(diagnostics::infer::member_not_found(
                            &struct_ty,
                            &field.name,
                            span,
                            Some(&available),
                            None,
                            false,
                        ));
                        Type::Error
                    }
                };

                let is_shorthand = matches!(
                    &field.value,
                    Pattern::Identifier { identifier, .. } if identifier == &field.name
                );
                let (inferred_value, typed_value) =
                    self.infer_pattern_inner(field.value.clone(), field_ty, kind, is_shorthand);
                (
                    StructFieldPattern {
                        name: field.name.clone(),
                        value: inferred_value,
                    },
                    (field.name.clone(), typed_value),
                )
            })
            .unzip();

        if !rest {
            let pattern_field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
            let missing: Vec<String> = struct_fields
                .iter()
                .filter(|sf| !pattern_field_names.contains(&sf.name.as_str()))
                .map(|sf| sf.name.to_string())
                .collect();
            if !missing.is_empty() {
                self.sink
                    .push(diagnostics::infer::pattern_missing_fields(&missing, span));
            }
        }

        let resolved_ty = struct_ty.resolve_in(&self.env);
        let typed = match &resolved_ty {
            Type::Nominal { id, params, .. } => TypedPattern::Struct {
                struct_name: id.into(),
                struct_fields,
                pattern_fields: typed_field_values,
                type_args: params.clone(),
            },
            _ => TypedPattern::Wildcard,
        };

        let pattern = Pattern::Struct {
            identifier,
            fields: new_fields,
            rest,
            ty: struct_ty,
            span,
        };
        (pattern, typed)
    }

    fn infer_or_pattern(
        &mut self,
        patterns: Vec<Pattern>,
        span: Span,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        let (first, first_typed) = self.infer_pattern_inner(
            patterns
                .first()
                .cloned()
                .unwrap_or(Pattern::WildCard { span }),
            expected_ty.clone(),
            kind,
            false,
        );
        let first_bindings = collect_pattern_bindings(&first);
        let first_names: HashSet<&str> = first_bindings
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();

        let first_binding_types: HashMap<String, Type> = first_bindings
            .iter()
            .filter_map(|(name, _)| {
                self.scopes
                    .lookup_value(name)
                    .map(|ty| (name.clone(), ty.clone()))
            })
            .collect();

        let mut inferred = vec![first];
        let mut typed_alternatives = vec![first_typed];

        for pattern in patterns.iter().skip(1) {
            self.scopes.push();
            let checkpoint = self.facts.binding_checkpoint();
            let (alt, alt_typed) =
                self.infer_pattern_inner(pattern.clone(), expected_ty.clone(), kind, false);
            let alt_bindings = collect_pattern_bindings(&alt);
            let alt_names: HashSet<&str> =
                alt_bindings.iter().map(|(name, _)| name.as_str()).collect();

            if first_names != alt_names {
                let missing_in_alt: Vec<&str> =
                    first_names.difference(&alt_names).copied().collect();
                let missing_in_first: Vec<&str> =
                    alt_names.difference(&first_names).copied().collect();

                let error_span = if let Some(name) = missing_in_alt.first() {
                    first_bindings
                        .iter()
                        .find(|(n, _)| n == *name)
                        .map(|(_, s)| *s)
                } else if let Some(name) = missing_in_first.first() {
                    alt_bindings
                        .iter()
                        .find(|(n, _)| n == *name)
                        .map(|(_, s)| *s)
                } else {
                    None
                };

                self.sink
                    .push(diagnostics::infer::or_pattern_binding_mismatch(
                        error_span.unwrap_or(span),
                        &missing_in_alt,
                        &missing_in_first,
                    ));
                self.facts.or_pattern_error_spans.insert(span);
            } else {
                for (name, alt_span) in &alt_bindings {
                    if let Some(first_ty) = first_binding_types.get(name)
                        && let Some(alt_ty) = self.scopes.lookup_value(name)
                    {
                        let first_resolved = first_ty.resolve_in(&self.env);
                        let alt_resolved = alt_ty.resolve_in(&self.env);
                        if first_resolved != alt_resolved {
                            self.sink.push(diagnostics::infer::or_pattern_type_mismatch(
                                *alt_span,
                                &first_resolved.to_string(),
                                &alt_resolved.to_string(),
                            ));
                        }
                    }
                }
            }
            self.scopes.pop();
            self.facts.remove_bindings_from(checkpoint);
            inferred.push(alt);
            typed_alternatives.push(alt_typed);
        }

        let pattern = Pattern::Or {
            patterns: inferred,
            span,
        };
        let typed = TypedPattern::Or {
            alternatives: typed_alternatives,
        };
        (pattern, typed)
    }

    fn get_enum_variant_info(&self, ty: &Type) -> Option<(String, Vec<String>)> {
        let store = self.store;
        let resolved = ty.resolve_in(&self.env);
        let Type::Nominal { id: display_id, .. } = &resolved else {
            return None;
        };
        let enum_ty = self.peel_to_enum(&resolved)?;
        let Type::Nominal { id: enum_id, .. } = &enum_ty else {
            return None;
        };
        let variants = store.variants_of(enum_id.as_str())?;
        let variant_names: Vec<String> = variants.iter().map(|v| v.name.to_string()).collect();
        let display_name = self.enum_display_name(display_id.as_str());
        Some((display_name, variant_names))
    }

    fn enum_display_name(&self, id: &str) -> String {
        let store = self.store;
        let simple = unqualified_name(id);
        let Some(module_id) = store.module_for_qualified_name(id) else {
            return simple.to_string();
        };
        if module_id == self.cursor.module_id || self.imports.unprefixed_imports.contains(module_id)
        {
            return simple.to_string();
        }
        for (prefix, imported_module_id) in &self.imports.prefix_to_module {
            if imported_module_id == module_id {
                return format!("{}.{}", prefix, simple);
            }
        }
        simple.to_string()
    }

    /// Tries to resolve an identifier like `api.UIEvent.Click` through a type alias.
    ///
    /// Returns the variant constructor type and the variant name if successful.
    /// For tuple variants, returns the function type (e.g., `fn(string) -> Event`).
    /// For unit variants, returns the enum type directly.
    fn try_resolve_type_alias_variant(&mut self, identifier: &str) -> Option<(Type, String)> {
        let store = self.store;
        let (type_part, variant_name) = identifier.rsplit_once('.')?;

        let qualified_name = self.lookup_qualified_name(store, type_part)?;
        let def = store.get_definition(&qualified_name)?;
        let DefinitionBody::TypeAlias { .. } = &def.body else {
            return None;
        };
        let alias_ty = &def.ty;

        let underlying = match alias_ty {
            Type::Forall { body, .. } => body.as_ref().clone(),
            _ => alias_ty.clone(),
        };
        let underlying = store.deep_resolve_alias(&underlying);

        if let Type::Nominal { id: enum_id, .. } = &underlying
            && let Some(variants) = store.variants_of(enum_id.as_str())
            && let Some(variant) = variants.iter().find(|v| v.name == variant_name)
        {
            let variant_qualified_name = enum_id.with_segment(variant_name);
            if let Some(variant_ty) = store.get_type(&variant_qualified_name) {
                return Some((variant_ty.clone(), variant_name.to_string()));
            }
            if variant.fields.is_empty() {
                return Some((underlying.clone(), variant_name.to_string()));
            }
        }

        None
    }

    fn peel_to_enum(&self, ty: &Type) -> Option<Type> {
        let store = self.store;
        let resolved = store.deep_resolve_alias(&ty.resolve_in(&self.env));
        match &resolved {
            Type::Nominal { id, .. } if store.variants_of(id.as_str()).is_some() => Some(resolved),
            _ => None,
        }
    }

    fn resolve_bare_variant_type(&self, identifier: &str, expected_ty: &Type) -> Option<Type> {
        let store = self.store;
        if identifier.contains('.') || !identifier.chars().next().is_some_and(char::is_uppercase) {
            return None;
        }
        let resolved = self.peel_to_enum(expected_ty)?;
        let Type::Nominal { id, .. } = &resolved else {
            return None;
        };
        let variant = store
            .variants_of(id.as_str())?
            .iter()
            .find(|v| v.name == identifier)?;
        let variant_qualified = id.with_segment(identifier);
        if let Some(ty) = store.get_type(&variant_qualified) {
            return Some(ty.clone());
        }
        variant.fields.is_empty().then(|| resolved.clone())
    }

    /// Tries to infer an enum struct variant pattern like `Move { x, y }`.
    #[allow(clippy::too_many_arguments)]
    fn try_infer_enum_struct_variant(
        &mut self,
        identifier: &str,
        fields: &[StructFieldPattern],
        rest: bool,
        span: &Span,
        expected_ty: &Type,
        kind: BindingKind,
    ) -> Option<(Pattern, TypedPattern)> {
        let store = self.store;
        let bare_variant = if kind.is_match_arm() {
            self.resolve_bare_variant_type(identifier, expected_ty)
                .map(|ty| (ty, unqualified_name(identifier).to_string()))
        } else {
            None
        };

        let (ty, variant_name) = if let Some(resolved) = bare_variant {
            resolved
        } else if let Some(ty) = self.lookup_type(store, identifier) {
            let variant_name = unqualified_name(identifier);
            (ty, variant_name.to_string())
        } else if let Some((alias_ty, variant_name)) =
            self.try_resolve_type_alias_variant(identifier)
        {
            (alias_ty, variant_name)
        } else {
            return None;
        };

        let (value_constructor_type, map) = self.instantiate(&ty);

        let pattern_ty = match value_constructor_type {
            Type::Function(f) => (*f.return_type).clone(),
            Type::Nominal { .. } => value_constructor_type,
            _ => return None,
        };

        let unify_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        self.unify(&unify_expected, &pattern_ty, span);

        let resolved_ty = pattern_ty.resolve_in(&self.env);

        let Type::Nominal { id, .. } = &resolved_ty else {
            return None;
        };
        let variants = store.variants_of(id)?;
        let variant = variants.iter().find(|v| v.name == variant_name)?;
        if !variant.fields.is_struct() {
            return None;
        }

        let variant_fields: Vec<EnumFieldDefinition> = variant.fields.iter().cloned().collect();
        let available: Vec<String> = variant_fields.iter().map(|f| f.name.to_string()).collect();

        let (new_fields, typed_field_values): (Vec<_>, Vec<_>) = fields
            .iter()
            .map(|field| {
                let field_definition = variant_fields.iter().find(|x| x.name == field.name);
                let field_ty = match field_definition {
                    Some(field_definition) => substitute(&field_definition.ty, &map),
                    None => {
                        self.sink.push(diagnostics::infer::member_not_found(
                            &pattern_ty,
                            &field.name,
                            *span,
                            Some(&available),
                            None,
                            false,
                        ));
                        Type::Error
                    }
                };

                let is_shorthand = matches!(
                    &field.value,
                    Pattern::Identifier { identifier, .. } if identifier == &field.name
                );
                let (inferred_value, typed_value) =
                    self.infer_pattern_inner(field.value.clone(), field_ty, kind, is_shorthand);
                (
                    StructFieldPattern {
                        name: field.name.clone(),
                        value: inferred_value,
                    },
                    (field.name.clone(), typed_value),
                )
            })
            .unzip();

        if !rest {
            let pattern_field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
            let missing: Vec<String> = variant_fields
                .iter()
                .filter(|vf| !pattern_field_names.contains(&vf.name.as_str()))
                .map(|vf| vf.name.to_string())
                .collect();
            if !missing.is_empty() {
                self.sink
                    .push(diagnostics::infer::pattern_missing_fields(&missing, *span));
            }
        }

        let typed = match &resolved_ty {
            Type::Nominal { id, params, .. } => {
                let variant_qualified = id.with_segment(&variant_name);
                if let Some(definition_span) =
                    self.get_definition_name_span(store, &variant_qualified)
                {
                    self.facts.add_usage(*span, definition_span);
                }

                TypedPattern::EnumStructVariant {
                    enum_name: id.into(),
                    variant_name: identifier.into(),
                    variant_fields,
                    pattern_fields: typed_field_values,
                    type_args: params.clone(),
                }
            }
            _ => TypedPattern::Wildcard,
        };

        let pattern = Pattern::Struct {
            identifier: identifier.into(),
            fields: new_fields,
            rest,
            ty: pattern_ty,
            span: *span,
        };
        Some((pattern, typed))
    }
}

fn format_literal(lit: &Literal) -> String {
    match lit {
        Literal::Integer { text, value } => text.as_ref().unwrap_or(&value.to_string()).clone(),
        Literal::Float { text, value } => text.as_ref().unwrap_or(&value.to_string()).clone(),
        Literal::Imaginary(v) => format!("{}i", v),
        Literal::Boolean(b) => b.to_string(),
        Literal::String { value, raw: true } => format!("r\"{}\"", value),
        Literal::String { value, raw: false } => format!("\"{}\"", value),
        Literal::Char(c) => format!("'{}'", c),
        Literal::FormatString(_) => "f\"...\"".to_string(),
        Literal::Slice(_) => "[...]".to_string(),
    }
}
