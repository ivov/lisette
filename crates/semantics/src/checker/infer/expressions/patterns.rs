use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;
use syntax::ast::BindingKind;
use syntax::ast::{
    EnumFieldDefinition, Expression, Literal, Pattern, RestPattern, Span, StructFieldPattern,
    TypedPattern,
};
use syntax::program::Definition;
use syntax::types::{Type, substitute};

use super::super::Checker;
use super::super::checks::check_duplicate_bindings;

pub(crate) fn collect_pattern_bindings(pattern: &Pattern) -> Vec<(String, Span)> {
    match pattern {
        Pattern::Identifier { identifier, span } => vec![(identifier.to_string(), *span)],
        Pattern::Tuple { elements, .. } => {
            elements.iter().flat_map(collect_pattern_bindings).collect()
        }
        Pattern::EnumVariant { fields, .. } => {
            fields.iter().flat_map(collect_pattern_bindings).collect()
        }
        Pattern::Struct { fields, .. } => fields
            .iter()
            .flat_map(|f| collect_pattern_bindings(&f.value))
            .collect(),
        Pattern::Slice { prefix, rest, .. } => {
            let mut bindings: Vec<_> = prefix.iter().flat_map(collect_pattern_bindings).collect();
            if let RestPattern::Bind { name, span } = rest {
                bindings.push((name.to_string(), *span));
            }
            bindings
        }
        Pattern::Or { patterns, .. } => patterns
            .first()
            .map(collect_pattern_bindings)
            .unwrap_or_default(),
        Pattern::AsBinding {
            pattern,
            name,
            span,
        } => {
            let mut bindings = collect_pattern_bindings(pattern);
            bindings.push((name.to_string(), *span));
            bindings
        }
        Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => vec![],
    }
}

impl Checker<'_, '_> {
    pub(super) fn infer_pattern(
        &mut self,
        pattern: Pattern,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        check_duplicate_bindings(self.sink, &pattern);
        self.infer_pattern_inner(pattern, expected_ty, kind, false)
    }

    fn infer_pattern_inner(
        &mut self,
        pattern: Pattern,
        expected_ty: Type,
        kind: BindingKind,
        is_struct_field: bool,
    ) -> (Pattern, TypedPattern) {
        match pattern {
            Pattern::Identifier { identifier, span } => {
                self.bind_name_in_scope(
                    identifier.to_string(),
                    span,
                    expected_ty,
                    kind,
                    self.is_d_lis(),
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
                self.unify(&expected_ty, &self.type_unit(), &span);
                (
                    Pattern::Unit {
                        ty: self.type_unit(),
                        span,
                    },
                    TypedPattern::Wildcard,
                )
            }

            Pattern::Slice {
                prefix, rest, span, ..
            } => {
                let resolved_ty = expected_ty.resolve();
                let element_ty = match &resolved_ty {
                    Type::Constructor { id, params, .. }
                        if id.ends_with("Slice") && params.len() == 1 =>
                    {
                        params[0].clone()
                    }
                    _ => {
                        let element_ty = self.new_type_var();
                        let slice_ty = Type::Constructor {
                            id: "Slice".into(),
                            params: vec![element_ty.clone()],
                            underlying_ty: None,
                        };

                        self.unify(&expected_ty, &slice_ty, &span);

                        element_ty
                    }
                };

                let (inferred_prefix, typed_prefix): (Vec<_>, Vec<_>) = prefix
                    .into_iter()
                    .map(|p| self.infer_pattern_inner(p, element_ty.clone(), kind, false))
                    .unzip();

                if let RestPattern::Bind { ref name, ref span } = rest {
                    let rest_ty = self.type_slice(element_ty.clone());
                    let is_typedef = self.is_d_lis();
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

    fn infer_enum_variant_pattern(
        &mut self,
        identifier: EcoString,
        fields: Vec<Pattern>,
        rest: bool,
        span: Span,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        let is_bare_name = fields.is_empty() && !identifier.contains('.') && !kind.is_match_arm();

        let ty = if let Some(ty) = self.lookup_type(&identifier) {
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
            let bare_name = identifier.rsplit('.').next().unwrap_or(&identifier);
            self.sink
                .push(diagnostics::infer::enum_variant_constructor_not_found(
                    span,
                    enum_info.as_ref().map(|(n, v)| (n.as_str(), v.as_slice())),
                    bare_name,
                ));
            return (Pattern::WildCard { span }, TypedPattern::Wildcard);
        };

        let (value_constructor_type, _) = self.instantiate(&ty);

        if is_bare_name
            && matches!(&value_constructor_type, Type::Constructor { .. })
            && !self.is_enum_type(&value_constructor_type)
        {
            self.sink
                .push(diagnostics::infer::uppercase_binding(span, &identifier));
            return (Pattern::WildCard { span }, TypedPattern::Wildcard);
        }

        let (pattern_ty, params) = match value_constructor_type {
            Type::Function {
                params,
                return_type,
                ..
            } => (*return_type, params),
            Type::Constructor { .. } => (value_constructor_type, vec![]),
            _ => unreachable!(),
        };

        self.unify(&expected_ty, &pattern_ty, &span);

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

        let resolved_field_types: Box<[Type]> = params.iter().map(|p| p.resolve()).collect();

        let resolved_ty = pattern_ty.resolve();
        let typed = match &resolved_ty {
            Type::Constructor { id, params, .. } => {
                let variant_name = identifier.rsplit('.').next().unwrap_or(identifier.as_str());
                let variant_qualified = format!("{}.{}", id, variant_name);
                if let Some(definition_span) = self.get_definition_name_span(&variant_qualified) {
                    self.facts.add_usage(span, definition_span);
                }

                let variant_fields = self
                    .store
                    .get_definition(id)
                    .and_then(|definition| match definition {
                        Definition::Enum { variants, .. } => variants
                            .iter()
                            .find(|v| v.name == variant_name)
                            .map(|v| v.fields.iter().cloned().collect()),
                        _ => None,
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

    fn infer_struct_pattern(
        &mut self,
        identifier: EcoString,
        fields: Vec<StructFieldPattern>,
        rest: bool,
        span: Span,
        expected_ty: Type,
        kind: BindingKind,
    ) -> (Pattern, TypedPattern) {
        let Some(qualified_name) = self.lookup_qualified_name(&identifier) else {
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
        let Some(Definition::Struct {
            ty: struct_forall_ty,
            fields: definition_struct_fields,
            ..
        }) = self.store.get_definition(&qualified_name)
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

        self.track_name_usage(&qualified_name, &span, identifier.len() as u32);

        let (struct_ty, map) = self.instantiate(&struct_forall_ty);

        self.unify(&expected_ty, &struct_ty, &span);

        let struct_module = qualified_name.split('.').next().unwrap_or(&qualified_name);
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
                        substitute(&field_definition.ty, &map)
                    }
                    None => {
                        self.sink.push(diagnostics::infer::member_not_found(
                            &struct_ty,
                            &field.name,
                            span,
                            Some(&available),
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

        let resolved_ty = struct_ty.resolve();
        let typed = match &resolved_ty {
            Type::Constructor { id, params, .. } => TypedPattern::Struct {
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
                        let first_resolved = first_ty.resolve();
                        let alt_resolved = alt_ty.resolve();
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
        let resolved = ty.resolve();
        let Type::Constructor { id, .. } = resolved else {
            return None;
        };
        let variants = self.store.get_enum_variants(&id)?;
        let variant_names: Vec<String> = variants.iter().map(|v| v.name.to_string()).collect();
        let simple_name = id.rsplit('.').next().unwrap_or(&id);
        Some((simple_name.to_string(), variant_names))
    }

    /// Tries to resolve an identifier like `api.UIEvent.Click` through a type alias.
    ///
    /// Returns the variant constructor type and the variant name if successful.
    /// For tuple variants, returns the function type (e.g., `fn(string) -> Event`).
    /// For unit variants, returns the enum type directly.
    fn try_resolve_type_alias_variant(&mut self, identifier: &str) -> Option<(Type, String)> {
        let (type_part, variant_name) = identifier.rsplit_once('.')?;

        let qualified_name = self.lookup_qualified_name(type_part)?;
        let Definition::TypeAlias { ty: alias_ty, .. } =
            self.store.get_definition(&qualified_name)?
        else {
            return None;
        };

        let underlying = match &alias_ty {
            Type::Forall { body, .. } => body.as_ref().clone(),
            _ => alias_ty.clone(),
        };

        if let Type::Constructor { id: enum_id, .. } = &underlying
            && let Some(Definition::Enum { variants, .. }) = self.store.get_definition(enum_id)
            && let Some(variant) = variants.iter().find(|v| v.name == variant_name)
        {
            let variant_qualified_name = format!("{}.{}", enum_id, variant_name);
            if let Some(variant_ty) = self.store.get_type(&variant_qualified_name) {
                return Some((variant_ty.clone(), variant_name.to_string()));
            }
            if variant.fields.is_empty() {
                return Some((underlying.clone(), variant_name.to_string()));
            }
        }

        None
    }

    /// Tries to infer an enum struct variant pattern like `Move { x, y }`.
    fn try_infer_enum_struct_variant(
        &mut self,
        identifier: &str,
        fields: &[StructFieldPattern],
        rest: bool,
        span: &Span,
        expected_ty: &Type,
        kind: BindingKind,
    ) -> Option<(Pattern, TypedPattern)> {
        let (ty, variant_name) = if let Some(ty) = self.lookup_type(identifier) {
            let variant_name = identifier.split('.').next_back().unwrap_or(identifier);
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
            Type::Function { return_type, .. } => *return_type,
            Type::Constructor { .. } => value_constructor_type,
            _ => return None,
        };

        self.unify(expected_ty, &pattern_ty, span);

        let resolved_ty = pattern_ty.resolve();

        let Type::Constructor { id, .. } = &resolved_ty else {
            return None;
        };
        let Definition::Enum { variants, .. } = self.store.get_definition(id)? else {
            return None;
        };
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
            Type::Constructor { id, params, .. } => {
                let variant_qualified = format!("{}.{}", id, variant_name);
                if let Some(definition_span) = self.get_definition_name_span(&variant_qualified) {
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
        Literal::String(s) => format!("\"{}\"", s),
        Literal::Char(c) => format!("'{}'", c),
        Literal::FormatString(_) => "f\"...\"".to_string(),
        Literal::Slice(_) => "[...]".to_string(),
    }
}
