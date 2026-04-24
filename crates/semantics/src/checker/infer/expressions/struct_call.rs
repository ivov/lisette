use rustc_hash::FxHashSet as HashSet;

use crate::checker::EnvResolve;
use crate::store::Store;
use ecow::EcoString;
use syntax::ast::{Expression, Span, StructFieldAssignment};
use syntax::program::Definition;
use syntax::types::{SubstitutionMap, Type, substitute};

use super::super::TaskState;

impl TaskState<'_> {
    pub(super) fn infer_struct_call(
        &mut self,
        store: &mut Store,
        struct_name: EcoString,
        field_assignments: Vec<StructFieldAssignment>,
        spread: Box<Option<Expression>>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if let Some(qualified_name) = self.lookup_qualified_name(store, &struct_name)
            && let Some(Definition::Struct {
                ty: struct_ty,
                fields: struct_fields,
                ..
            }) = store.get_definition(&qualified_name)
        {
            let struct_ty = struct_ty.clone();
            let struct_fields = struct_fields.clone();

            self.track_name_usage(store, &qualified_name, &span, struct_name.len() as u32);
            return self.infer_struct_call_for_struct(
                store,
                struct_name,
                qualified_name,
                struct_ty,
                struct_fields,
                field_assignments,
                spread,
                span,
                expected_ty,
            );
        }

        if let Some(qualified_name) = self.lookup_qualified_name(store, &struct_name)
            && let Some(Definition::TypeAlias {
                ty: alias_ty,
                annotation,
                ..
            }) = store.get_definition(&qualified_name)
        {
            let alias_ty = alias_ty.clone();
            let is_opaque = annotation.is_opaque();

            let underlying = match &alias_ty {
                Type::Forall { body, .. } => body.as_ref().clone(),
                _ => alias_ty.clone(),
            };
            if let Type::Nominal { id: struct_id, .. } = &underlying
                && let Some(Definition::Struct {
                    ty: struct_ty,
                    fields: struct_fields,
                    ..
                }) = store.get_definition(struct_id)
            {
                let struct_ty = struct_ty.clone();
                let struct_fields = struct_fields.clone();
                let struct_id_str = struct_id.to_string();
                return self.infer_struct_call_for_struct(
                    store,
                    struct_name,
                    struct_id_str,
                    struct_ty,
                    struct_fields,
                    field_assignments,
                    spread,
                    span,
                    expected_ty,
                );
            }

            // Opaque types (e.g., Go's sync.WaitGroup) can be zero-value instantiated
            // with T{} even though they have no struct definition.
            if is_opaque && field_assignments.is_empty() {
                let (instantiated_ty, _) = self.instantiate(&alias_ty);
                self.unify(store, expected_ty, &instantiated_ty, &span);
                return Expression::StructCall {
                    name: struct_name,
                    field_assignments,
                    spread,
                    ty: instantiated_ty,
                    span,
                };
            }
        }

        if let Some((type_part, variant_name)) = struct_name.rsplit_once('.')
            && let Some(qualified_name) = self.lookup_qualified_name(store, type_part)
            && let Some(Definition::TypeAlias { ty: alias_ty, .. }) =
                store.get_definition(&qualified_name)
        {
            let alias_ty = alias_ty.clone();

            let underlying = match &alias_ty {
                Type::Forall { body, .. } => body.as_ref().clone(),
                _ => alias_ty.clone(),
            };
            let variant_fields = if let Type::Nominal { id: enum_id, .. } = &underlying
                && let Some(variants) = store.variants_of(enum_id)
                && let Some(variant) = variants.iter().find(|v| v.name == variant_name)
                && variant.fields.is_struct()
            {
                Some(variant.fields.iter().cloned().collect::<Vec<_>>())
            } else {
                None
            };

            if let Some(variant_fields) = variant_fields {
                let (instantiated_ty, map) = self.instantiate(&alias_ty);
                let enum_ty = match instantiated_ty {
                    Type::Function { return_type, .. } => *return_type,
                    _ => instantiated_ty,
                };
                return self.infer_struct_call_for_enum_variant(
                    store,
                    struct_name,
                    variant_fields,
                    map,
                    field_assignments,
                    spread,
                    span,
                    expected_ty,
                    enum_ty,
                );
            }
        }

        if let Some(ty) = self.lookup_type(store, &struct_name) {
            let (value_constructor_type, map) = self.instantiate(&ty);

            let pattern_ty = match value_constructor_type {
                Type::Function { return_type, .. } => *return_type,
                Type::Nominal { .. } => value_constructor_type,
                _ => {
                    self.sink
                        .push(diagnostics::infer::struct_not_found(&struct_name, span));
                    self.unify(store, expected_ty, &Type::Error, &span);
                    return Expression::StructCall {
                        name: struct_name,
                        field_assignments,
                        spread,
                        ty: Type::Error,
                        span,
                    };
                }
            };

            let resolved_ty = pattern_ty.resolve_in(&self.env);
            let variant_name = struct_name.split('.').next_back().unwrap_or(&struct_name);

            if let Type::Nominal { id, .. } = &resolved_ty
                && let Some(variants) = store.variants_of(id)
                && let Some(variant) = variants.iter().find(|v| v.name == variant_name)
                && variant.fields.is_struct()
            {
                let variant_fields: Vec<_> = variant.fields.iter().cloned().collect();
                return self.infer_struct_call_for_enum_variant(
                    store,
                    struct_name,
                    variant_fields,
                    map,
                    field_assignments,
                    spread,
                    span,
                    expected_ty,
                    pattern_ty,
                );
            }
        }

        self.sink
            .push(diagnostics::infer::struct_not_found(&struct_name, span));
        self.unify(store, expected_ty, &Type::Error, &span);
        Expression::StructCall {
            name: struct_name,
            field_assignments,
            spread,
            ty: Type::Error,
            span,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_struct_call_for_struct(
        &mut self,
        store: &mut Store,
        struct_name: EcoString,
        qualified_name: String,
        struct_ty: Type,
        struct_fields: Vec<syntax::ast::StructFieldDefinition>,
        field_assignments: Vec<StructFieldAssignment>,
        spread: Box<Option<Expression>>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let (struct_call_ty, map) = self.instantiate(&struct_ty);

        let new_spread = (*spread).map(|s| {
            self.with_value_context(|checker| checker.infer_expression(store, s, &struct_call_ty))
        });

        let struct_module = qualified_name.split('.').next().unwrap_or(&qualified_name);
        let is_cross_module = struct_module != self.cursor.module_id
            || struct_name
                .split_once('.')
                .is_some_and(|(prefix, _)| self.imports.imported_modules.contains_key(prefix));

        let mut matched_fields = HashSet::default();
        let new_field_assignments = field_assignments
            .iter()
            .map(|field| {
                let field_definition = struct_fields.iter().find(|f| f.name == field.name);

                let field_ty = match field_definition {
                    Some(field_definition) => {
                        matched_fields.insert(field.name.clone());

                        if is_cross_module && !field_definition.visibility.is_public() {
                            self.sink.push(diagnostics::infer::private_field_access(
                                &field.name,
                                &struct_name,
                                field.name_span,
                            ));
                        }

                        substitute(&field_definition.ty, &map)
                    }
                    None => {
                        let available: Vec<String> =
                            struct_fields.iter().map(|f| f.name.to_string()).collect();
                        self.sink.push(diagnostics::infer::member_not_found(
                            &struct_call_ty,
                            &field.name,
                            span,
                            Some(&available),
                        ));
                        self.new_type_var()
                    }
                };

                let new_value = self.with_value_context(|s| {
                    s.infer_expression(store, (*field.value).clone(), &field_ty)
                });

                StructFieldAssignment {
                    name: field.name.clone(),
                    name_span: field.name_span,
                    value: Box::new(new_value),
                }
            })
            .collect();

        if matched_fields.len() != struct_fields.len() && new_spread.is_none() {
            let mut missing_fields: Vec<String> = struct_fields
                .iter()
                .filter(|f| !matched_fields.contains(f.name.as_str()))
                .map(|f| f.name.to_string())
                .collect();

            missing_fields.sort();

            self.sink.push(diagnostics::infer::struct_missing_fields(
                &struct_name,
                &missing_fields,
                span,
            ));
        }

        if let Some(ref spread_expression) = new_spread
            && is_cross_module
        {
            for field in &struct_fields {
                if !matched_fields.contains(&field.name) && !field.visibility.is_public() {
                    self.sink.push(diagnostics::infer::private_field_in_spread(
                        &field.name,
                        &struct_name,
                        spread_expression.get_span(),
                    ));
                    break;
                }
            }
        }

        self.unify(store, expected_ty, &struct_call_ty, &span);

        Expression::StructCall {
            name: struct_name,
            field_assignments: new_field_assignments,
            spread: new_spread.into(),
            ty: struct_call_ty,
            span,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_struct_call_for_enum_variant(
        &mut self,
        store: &mut Store,
        variant_name: EcoString,
        variant_fields: Vec<syntax::ast::EnumFieldDefinition>,
        map: SubstitutionMap,
        field_assignments: Vec<StructFieldAssignment>,
        spread: Box<Option<Expression>>,
        span: Span,
        expected_ty: &Type,
        enum_ty: Type,
    ) -> Expression {
        self.unify(store, expected_ty, &enum_ty, &span);

        let new_spread = (*spread).map(|s| {
            self.with_value_context(|checker| checker.infer_expression(store, s, &enum_ty))
        });

        let mut matched_fields = HashSet::default();
        let new_field_assignments: Vec<StructFieldAssignment> = field_assignments
            .iter()
            .map(|field| {
                let field_definition = variant_fields.iter().find(|f| f.name == field.name);

                let field_ty = match field_definition {
                    Some(field_definition) => {
                        matched_fields.insert(field.name.clone());
                        substitute(&field_definition.ty, &map)
                    }
                    None => {
                        let available: Vec<String> =
                            variant_fields.iter().map(|f| f.name.to_string()).collect();
                        self.sink.push(diagnostics::infer::member_not_found(
                            &enum_ty,
                            &field.name,
                            span,
                            Some(&available),
                        ));
                        self.new_type_var()
                    }
                };

                let new_value = self.with_value_context(|s| {
                    s.infer_expression(store, (*field.value).clone(), &field_ty)
                });

                StructFieldAssignment {
                    name: field.name.clone(),
                    name_span: field.name_span,
                    value: Box::new(new_value),
                }
            })
            .collect();

        if matched_fields.len() != variant_fields.len() && new_spread.is_none() {
            let mut missing_fields: Vec<String> = variant_fields
                .iter()
                .filter(|f| !matched_fields.contains(f.name.as_str()))
                .map(|f| f.name.to_string())
                .collect();

            missing_fields.sort();

            self.sink.push(diagnostics::infer::struct_missing_fields(
                &variant_name,
                &missing_fields,
                span,
            ));
        }

        Expression::StructCall {
            name: variant_name,
            field_assignments: new_field_assignments,
            spread: new_spread.into(),
            ty: enum_ty,
            span,
        }
    }
}
