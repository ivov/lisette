use rustc_hash::FxHashSet as HashSet;

use crate::checker::EnvResolve;
use crate::zero::{NoZero, NoZeroReason};
use ecow::EcoString;
use syntax::ast::{Expression, Span, StructFieldAssignment, StructSpread};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{SubstitutionMap, Type, substitute, unqualified_name};

use crate::checker::infer::InferCtx;

/// Inputs to `infer_structish_fields` shared between struct and enum-variant literals.
struct StructishCtx<'a, 'b, F> {
    field_assignments: &'b [StructFieldAssignment],
    target_ty: &'b Type,
    owner_name: &'b str,
    spread: &'b StructSpread,
    span: Span,
    all_fields: F,
    map: &'b SubstitutionMap,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl InferCtx<'_, '_> {
    pub(super) fn infer_struct_call(
        &mut self,
        struct_name: EcoString,
        field_assignments: Vec<StructFieldAssignment>,
        spread: StructSpread,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        if let Some(qualified_name) = self.lookup_qualified_name(store, &struct_name)
            && let Some(Definition {
                ty: struct_ty,
                body:
                    DefinitionBody::Struct {
                        fields: struct_fields,
                        ..
                    },
                ..
            }) = store.get_definition(&qualified_name)
        {
            let struct_ty = struct_ty.clone();
            let struct_fields = struct_fields.clone();

            self.track_name_usage(store, &qualified_name, &span, struct_name.len() as u32);
            return self.infer_struct_call_for_struct(
                struct_name,
                qualified_name,
                struct_ty,
                struct_fields,
                field_assignments,
                spread,
                span,
                expected_ty,
                None,
            );
        }

        if let Some(qualified_name) = self.lookup_qualified_name(store, &struct_name)
            && let Some(Definition {
                ty: alias_ty,
                body: DefinitionBody::TypeAlias { annotation, .. },
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
                && let Some(Definition {
                    ty: struct_ty,
                    body:
                        DefinitionBody::Struct {
                            fields: struct_fields,
                            ..
                        },
                    ..
                }) = store.get_definition(struct_id)
            {
                let struct_ty = struct_ty.clone();
                let struct_fields = struct_fields.clone();
                let struct_id_str: EcoString = struct_id.into();
                let alias_underlying = if matches!(&alias_ty, Type::Forall { .. }) {
                    None
                } else {
                    Some(underlying)
                };
                return self.infer_struct_call_for_struct(
                    struct_name,
                    struct_id_str,
                    struct_ty,
                    struct_fields,
                    field_assignments,
                    spread,
                    span,
                    expected_ty,
                    alias_underlying,
                );
            }

            // Opaque types (e.g., Go's sync.WaitGroup) can be zero-value instantiated
            // with T{} even though they have no struct definition.
            if is_opaque && field_assignments.is_empty() {
                let (instantiated_ty, _) = self.instantiate(&alias_ty);
                self.unify(expected_ty, &instantiated_ty, &span);
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
            && let Some(Definition {
                ty: alias_ty,
                body: DefinitionBody::TypeAlias { .. },
                ..
            }) = store.get_definition(&qualified_name)
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
                    Type::Function(f) => (*f.return_type).clone(),
                    _ => instantiated_ty,
                };
                return self.infer_struct_call_for_enum_variant(
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
                Type::Function(f) => (*f.return_type).clone(),
                Type::Nominal { .. } => value_constructor_type,
                _ => {
                    self.sink
                        .push(diagnostics::infer::struct_not_found(&struct_name, span));
                    self.unify(expected_ty, &Type::Error, &span);
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
            let variant_name = unqualified_name(&struct_name);

            if let Type::Nominal { id, .. } = &resolved_ty
                && let Some(variants) = store.variants_of(id)
                && let Some(variant) = variants.iter().find(|v| v.name == variant_name)
                && variant.fields.is_struct()
            {
                let variant_fields: Vec<_> = variant.fields.iter().cloned().collect();
                return self.infer_struct_call_for_enum_variant(
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
        self.unify(expected_ty, &Type::Error, &span);
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
        struct_name: EcoString,
        qualified_name: EcoString,
        struct_ty: Type,
        struct_fields: Vec<syntax::ast::StructFieldDefinition>,
        field_assignments: Vec<StructFieldAssignment>,
        spread: StructSpread,
        span: Span,
        expected_ty: &Type,
        alias_underlying: Option<Type>,
    ) -> Expression {
        let store = self.store;
        let (struct_call_ty, map) = self.instantiate(&struct_ty);

        if let Some(underlying) = alias_underlying {
            self.unify(&struct_call_ty, &underlying, &span);
        }

        let peeled_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        if same_nominal(&peeled_expected, &struct_call_ty) && !peeled_expected.contains_unknown() {
            let _ = self.speculatively(|this| {
                InferCtx::new(this, store).try_unify(&peeled_expected, &struct_call_ty, &span)
            });
        }

        let new_spread = self.infer_struct_spread(spread, &struct_call_ty);

        let struct_module = store
            .module_for_qualified_name(&qualified_name)
            .unwrap_or(&qualified_name);
        let is_cross_module = struct_module != self.cursor.module_id
            || struct_name
                .split_once('.')
                .is_some_and(|(prefix, _)| self.imports.imported_modules.contains_key(prefix));
        let is_go_imported = qualified_name.starts_with("go:");

        let (new_field_assignments, matched_fields) = self.infer_structish_fields(
            StructishCtx {
                field_assignments: &field_assignments,
                target_ty: &struct_call_ty,
                owner_name: &struct_name,
                spread: &new_spread,
                span,
                all_fields: struct_fields.iter().map(|f| (&f.name, &f.ty)),
                map: &map,
                _marker: std::marker::PhantomData,
            },
            |checker, assignment| {
                let def = struct_fields.iter().find(|f| f.name == assignment.name)?;
                if is_cross_module && !def.visibility.is_public() {
                    checker.sink.push(diagnostics::infer::private_field_access(
                        &assignment.name,
                        &struct_name,
                        assignment.name_span,
                    ));
                }
                Some(&def.ty)
            },
        );

        if let StructSpread::ZeroFill { span: spread_span } = &new_spread
            && !is_go_imported
        {
            self.check_zero_fill_fields(
                &struct_name,
                struct_fields.iter().map(|f| (&f.name, &f.ty)),
                &matched_fields,
                &map,
                *spread_span,
            );
        }

        if let Some(spread_span) = new_spread.span()
            && is_cross_module
            && !is_go_imported
        {
            let owning_module = store
                .module_for_qualified_name(&qualified_name)
                .unwrap_or(&qualified_name);
            for field in &struct_fields {
                if !matched_fields.contains(&field.name) && !field.visibility.is_public() {
                    let diag = match &new_spread {
                        StructSpread::ZeroFill { .. } => {
                            diagnostics::infer::private_field_in_zero_fill(
                                &field.name,
                                &struct_name,
                                owning_module,
                                spread_span,
                            )
                        }
                        _ => diagnostics::infer::private_field_in_spread(
                            &field.name,
                            &struct_name,
                            spread_span,
                        ),
                    };
                    self.sink.push(diag);
                    break;
                }
            }
        }

        let final_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        self.unify(&final_expected, &struct_call_ty, &span);

        Expression::StructCall {
            name: struct_name,
            field_assignments: new_field_assignments,
            spread: new_spread,
            ty: struct_call_ty,
            span,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_struct_call_for_enum_variant(
        &mut self,
        variant_name: EcoString,
        variant_fields: Vec<syntax::ast::EnumFieldDefinition>,
        map: SubstitutionMap,
        field_assignments: Vec<StructFieldAssignment>,
        spread: StructSpread,
        span: Span,
        expected_ty: &Type,
        enum_ty: Type,
    ) -> Expression {
        let store = self.store;
        self.unify(expected_ty, &enum_ty, &span);

        let resolved_enum = enum_ty.resolve_in(&self.env);
        if let Type::Nominal { id, .. } = &resolved_enum {
            let variant_last = unqualified_name(&variant_name);
            let qualified = id.with_segment(variant_last).to_string();
            self.track_name_usage(store, &qualified, &span, span.byte_length);
        }

        let new_spread = self.infer_struct_spread(spread, &enum_ty);

        let (new_field_assignments, matched_fields) = self.infer_structish_fields(
            StructishCtx {
                field_assignments: &field_assignments,
                target_ty: &enum_ty,
                owner_name: &variant_name,
                spread: &new_spread,
                span,
                all_fields: variant_fields.iter().map(|f| (&f.name, &f.ty)),
                map: &map,
                _marker: std::marker::PhantomData,
            },
            |_checker, assignment| {
                variant_fields
                    .iter()
                    .find(|f| f.name == assignment.name)
                    .map(|f| &f.ty)
            },
        );

        if let StructSpread::ZeroFill { span: spread_span } = &new_spread {
            self.check_zero_fill_fields(
                &variant_name,
                variant_fields.iter().map(|f| (&f.name, &f.ty)),
                &matched_fields,
                &map,
                *spread_span,
            );
        }

        Expression::StructCall {
            name: variant_name,
            field_assignments: new_field_assignments,
            spread: new_spread,
            ty: enum_ty,
            span,
        }
    }

    fn infer_struct_spread(&mut self, spread: StructSpread, target_ty: &Type) -> StructSpread {
        match spread {
            StructSpread::None => StructSpread::None,
            StructSpread::From(s) => {
                let inferred =
                    self.with_value_context(|checker| checker.infer_expression(*s, target_ty));
                StructSpread::From(Box::new(inferred))
            }
            StructSpread::ZeroFill { span } => StructSpread::ZeroFill { span },
        }
    }

    fn infer_structish_fields<'a, FindDef>(
        &mut self,
        ctx: StructishCtx<'a, '_, impl Iterator<Item = (&'a EcoString, &'a Type)> + Clone>,
        mut find_def: FindDef,
    ) -> (Vec<StructFieldAssignment>, HashSet<EcoString>)
    where
        FindDef: FnMut(&mut Self, &StructFieldAssignment) -> Option<&'a Type>,
    {
        let mut matched = HashSet::default();
        let new_assignments: Vec<StructFieldAssignment> = ctx
            .field_assignments
            .iter()
            .map(|field| {
                let field_ty = match find_def(self, field) {
                    Some(def_ty) => {
                        matched.insert(field.name.clone());
                        substitute(def_ty, ctx.map)
                    }
                    None => {
                        let available: Vec<String> =
                            ctx.all_fields.clone().map(|(n, _)| n.to_string()).collect();
                        self.sink.push(diagnostics::infer::member_not_found(
                            ctx.target_ty,
                            &field.name,
                            ctx.span,
                            Some(&available),
                            None,
                            false,
                        ));
                        self.new_type_var()
                    }
                };
                let new_value = self
                    .with_value_context(|s| s.infer_expression((*field.value).clone(), &field_ty));
                StructFieldAssignment {
                    name: field.name.clone(),
                    name_span: field.name_span,
                    value: Box::new(new_value),
                }
            })
            .collect();

        if ctx.spread.is_none() {
            let mut missing: Vec<String> = ctx
                .all_fields
                .clone()
                .filter(|(n, _)| !matched.contains(n.as_str()))
                .map(|(n, _)| n.to_string())
                .collect();
            if !missing.is_empty() {
                missing.sort();
                self.sink.push(diagnostics::infer::struct_missing_fields(
                    ctx.owner_name,
                    &missing,
                    ctx.span,
                ));
            }
        }

        (new_assignments, matched)
    }

    fn check_zero_fill_fields<'a>(
        &mut self,
        owner_name: &str,
        fields: impl Iterator<Item = (&'a EcoString, &'a Type)>,
        matched_fields: &HashSet<EcoString>,
        map: &SubstitutionMap,
        spread_span: Span,
    ) {
        let from_module = self.cursor.module_id.clone();
        for (name, ty) in fields {
            if matched_fields.contains(name.as_str()) {
                continue;
            }
            let resolved = substitute(ty, map);
            let Err(no_zero) = self.has_zero(&resolved, &from_module) else {
                continue;
            };
            let chain: Vec<&str> = no_zero.chain.iter().map(EcoString::as_str).collect();
            let private = match &no_zero.reason {
                NoZeroReason::PrivateField {
                    struct_name: ps,
                    field: pf,
                    owning_module: pm,
                } => Some((ps.as_str(), pf.as_str(), pm.as_str())),
                NoZeroReason::NoZeroForType => None,
            };
            self.sink.push(diagnostics::infer::field_no_zero(
                owner_name,
                name,
                &no_zero.leaf_ty,
                &chain,
                private,
                spread_span,
            ));
        }
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn has_zero(&self, ty: &Type, from_module: &str) -> Result<(), NoZero> {
        let store = self.store;
        crate::zero::has_zero(store, ty, from_module)
    }
}

fn same_nominal(a: &Type, b: &Type) -> bool {
    matches!(
        (a, b),
        (Type::Nominal { id: ai, .. }, Type::Nominal { id: bi, .. }) if ai == bi
    )
}
