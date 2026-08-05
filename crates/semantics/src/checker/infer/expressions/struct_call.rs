use rustc_hash::FxHashSet as HashSet;

use crate::checker::EnvResolve;
use crate::zero::{NoZero, NoZeroReason};
use ecow::EcoString;
use syntax::ast::{Expression, Span, StructFieldAssignment, StructFields, StructSpread};
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

struct StructLiteral {
    name: EcoString,
    fields: Vec<StructFieldAssignment>,
    spread: StructSpread,
    span: Span,
}

impl StructLiteral {
    fn with_type(self, ty: Type) -> Expression {
        Expression::StructCall {
            name: self.name,
            field_assignments: self.fields,
            spread: self.spread,
            ty,
            span: self.span,
        }
    }
}

struct ResolvedStruct {
    qualified_name: EcoString,
    ty: Type,
    fields: StructFields,
    alias_underlying: Option<Type>,
}

struct ResolvedVariant {
    fields: Vec<syntax::ast::EnumFieldDefinition>,
    substitutions: SubstitutionMap,
    ty: Type,
}

impl InferCtx<'_> {
    pub(super) fn infer_struct_call(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        let Expression::StructCall {
            name,
            field_assignments,
            spread,
            span,
            ..
        } = expression
        else {
            unreachable!("infer_struct_call called with non-StructCall expression");
        };
        let literal = StructLiteral {
            name,
            fields: field_assignments,
            spread,
            span,
        };
        let store = self.store;

        if let Some(resolved) = self
            .as_struct(store, &literal)
            .or_else(|| self.as_alias_struct(store, &literal.name))
        {
            return self.infer_struct_call_for_struct(literal, resolved, expected_ty);
        }

        // Opaque types (e.g., Go's sync.WaitGroup) can be zero-value instantiated
        // with T{} even though they have no struct definition.
        if let Some(qualified_name) = self.lookup_qualified_name(store, &literal.name)
            && let Some(Definition {
                ty: alias_ty,
                body: DefinitionBody::TypeAlias { alias, .. },
                ..
            }) = store.get_definition(&qualified_name)
            && matches!(alias, syntax::program::AliasKind::Opaque(_))
            && literal.fields.is_empty()
        {
            let alias_ty = alias_ty.clone();
            let (instantiated_ty, _) = self.instantiate(&alias_ty);
            self.unify(expected_ty, &instantiated_ty, &literal.span);
            let from_package = self.cursor.package_id.clone();
            if self.has_zero(&instantiated_ty, &from_package).is_err() {
                self.sink.push(diagnostics::infer::hidden_state_no_zero(
                    &instantiated_ty,
                    literal.span,
                ));
            }
            return literal.with_type(instantiated_ty);
        }

        if let Some(resolved) = self
            .as_alias_variant(store, &literal.name)
            .or_else(|| self.as_variant(store, &literal.name))
        {
            return self.infer_struct_call_for_enum_variant(literal, resolved, expected_ty);
        }

        if !self.may_name_uninferred_export(store, &literal.name) {
            self.sink.push(diagnostics::infer::struct_not_found(
                &literal.name,
                literal.span,
            ));
        }
        self.unify(expected_ty, &Type::Error, &literal.span);
        literal.with_type(Type::Error)
    }

    fn as_struct(
        &mut self,
        store: &crate::store::Store,
        literal: &StructLiteral,
    ) -> Option<ResolvedStruct> {
        let qualified_name = self.lookup_qualified_name(store, &literal.name)?;
        let Definition {
            ty: struct_ty,
            body:
                DefinitionBody::Struct {
                    fields: struct_fields,
                    ..
                },
            ..
        } = store.get_definition(&qualified_name)?
        else {
            return None;
        };
        let struct_ty = struct_ty.clone();
        let struct_fields = struct_fields.clone();

        self.track_name_usage(
            store,
            &qualified_name,
            &literal.span,
            literal.name.len() as u32,
        );
        Some(ResolvedStruct {
            qualified_name,
            ty: struct_ty,
            fields: struct_fields,
            alias_underlying: None,
        })
    }

    /// Resolve a `type Alias = Struct` name to its underlying struct.
    fn as_alias_struct(&self, store: &crate::store::Store, name: &str) -> Option<ResolvedStruct> {
        let qualified_name = self.lookup_qualified_name(store, name)?;
        let Definition {
            ty: alias_ty,
            body: DefinitionBody::TypeAlias { alias, .. },
            ..
        } = store.get_definition(&qualified_name)?
        else {
            return None;
        };
        let alias_ty = alias_ty.clone();
        let is_opaque = matches!(alias, syntax::program::AliasKind::Opaque(_));

        let underlying = (!is_opaque).then(|| store.peel_alias(&alias_ty));
        let Some(Type::Nominal { id: struct_id, .. }) = &underlying else {
            return None;
        };
        let Definition {
            ty: struct_ty,
            body:
                DefinitionBody::Struct {
                    fields: struct_fields,
                    ..
                },
            ..
        } = store.get_definition(struct_id)?
        else {
            return None;
        };
        let struct_ty = struct_ty.clone();
        let struct_fields = struct_fields.clone();
        let struct_id_str: EcoString = struct_id.into();
        // Deliberately None for a `Type::Forall` alias.
        let alias_underlying = if matches!(&alias_ty, Type::Forall { .. }) {
            None
        } else {
            underlying
        };
        Some(ResolvedStruct {
            qualified_name: struct_id_str,
            ty: struct_ty,
            fields: struct_fields,
            alias_underlying,
        })
    }

    fn as_alias_variant(
        &mut self,
        store: &crate::store::Store,
        name: &str,
    ) -> Option<ResolvedVariant> {
        let (type_part, variant_name) = name.rsplit_once('.')?;
        let qualified_name = self.lookup_qualified_name(store, type_part)?;
        let Definition {
            ty: alias_ty,
            body:
                DefinitionBody::TypeAlias {
                    alias: syntax::program::AliasKind::Transparent { .. },
                    ..
                },
            ..
        } = store.get_definition(&qualified_name)?
        else {
            return None;
        };
        let alias_ty = alias_ty.clone();
        let underlying = store.peel_alias(&alias_ty);
        let Type::Nominal { id: enum_id, .. } = &underlying else {
            return None;
        };
        let variants = store.variants_of(enum_id)?;
        let variant = variants.iter().find(|v| v.name == variant_name)?;
        if !variant.fields.is_struct() {
            return None;
        }
        let variant_fields: Vec<_> = variant.fields.iter().cloned().collect();

        let (instantiated_ty, map) = self.instantiate(&alias_ty);
        let instantiated_target = store.peel_alias(&instantiated_ty);
        let enum_ty = match instantiated_target {
            Type::Function(f) => (*f.return_type).clone(),
            other => other,
        };
        Some(ResolvedVariant {
            fields: variant_fields,
            substitutions: map,
            ty: enum_ty,
        })
    }

    fn as_variant(&mut self, store: &crate::store::Store, name: &str) -> Option<ResolvedVariant> {
        let ty = self.lookup_type(store, name)?;
        let (value_constructor_type, map) = self.instantiate(&ty);

        let pattern_ty = match value_constructor_type {
            Type::Function(f) => (*f.return_type).clone(),
            Type::Nominal { .. } => value_constructor_type,
            _ => return None,
        };

        let resolved_ty = pattern_ty.resolve_in(&self.env);
        let variant_name = unqualified_name(name);

        let Type::Nominal { id, .. } = &resolved_ty else {
            return None;
        };
        let variants = store.variants_of(id)?;
        let variant = variants.iter().find(|v| v.name == variant_name)?;
        if !variant.fields.is_struct() {
            return None;
        }
        let variant_fields: Vec<_> = variant.fields.iter().cloned().collect();
        Some(ResolvedVariant {
            fields: variant_fields,
            substitutions: map,
            ty: pattern_ty,
        })
    }

    fn infer_struct_call_for_struct(
        &mut self,
        literal: StructLiteral,
        target: ResolvedStruct,
        expected_ty: &Type,
    ) -> Expression {
        let StructLiteral {
            name: struct_name,
            fields: field_assignments,
            spread,
            span,
        } = literal;
        let ResolvedStruct {
            qualified_name,
            ty: struct_ty,
            fields: struct_fields,
            alias_underlying,
        } = target;
        let store = self.store;
        let (struct_call_ty, map) = self.instantiate(&struct_ty);

        if let Some(underlying) = alias_underlying {
            self.unify(&struct_call_ty, &underlying, &span);
        }

        let peeled_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        if same_nominal(&peeled_expected, &struct_call_ty)
            && !store.contains_unknown(&peeled_expected)
        {
            let _ = self.speculatively(|this| {
                InferCtx::new(this, store).try_unify(&peeled_expected, &struct_call_ty, &span)
            });
        }

        let new_spread = self.infer_struct_spread(spread, &struct_call_ty);

        let struct_package = store
            .package_for_qualified_name(&qualified_name)
            .unwrap_or(&qualified_name);
        let is_cross_package = struct_package != self.cursor.package_id
            || struct_name
                .split_once('.')
                .is_some_and(|(prefix, _)| self.imports.namespace(prefix).is_some());
        let is_go_imported = qualified_name.starts_with("go:");

        if is_go_imported
            && !matches!(new_spread, StructSpread::From(_))
            && let Some(def) = store.get_definition(&qualified_name)
            && crate::zero::go_struct_denies_zero(def, &struct_fields)
        {
            self.sink.push(diagnostics::infer::hidden_state_no_zero(
                &struct_call_ty,
                span,
            ));
        }

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
                if is_cross_package && !def.visibility.is_public() {
                    checker.sink.push(diagnostics::infer::private_field_access(
                        &assignment.name,
                        &struct_name,
                        struct_package,
                        assignment.name_span,
                    ));
                }
                Some(&def.ty)
            },
        );

        if let StructSpread::Autofill { span: spread_span } = &new_spread {
            self.check_autofill_fields(
                &struct_name,
                struct_fields
                    .iter()
                    .filter(|f| !(is_go_imported && crate::zero::hidden_embed_field(f)))
                    .map(|f| (&f.name, &f.ty)),
                &matched_fields,
                &map,
                *spread_span,
            );
        }

        if let Some(spread_span) = new_spread.span()
            && is_cross_package
            && !is_go_imported
        {
            let owning_package = store
                .package_for_qualified_name(&qualified_name)
                .unwrap_or(&qualified_name);
            for field in &struct_fields {
                if !matched_fields.contains(&field.name) && !field.visibility.is_public() {
                    let diag = match &new_spread {
                        StructSpread::Autofill { .. } => {
                            diagnostics::infer::private_field_in_autofill(
                                &field.name,
                                &struct_name,
                                owning_package,
                                spread_span,
                            )
                        }
                        _ => diagnostics::infer::private_field_in_spread(
                            &field.name,
                            &struct_name,
                            owning_package,
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

        self.register_construction_obligations(&struct_name, &struct_call_ty, span);

        Expression::StructCall {
            name: struct_name,
            field_assignments: new_field_assignments,
            spread: new_spread,
            ty: struct_call_ty,
            span,
        }
    }

    fn infer_struct_call_for_enum_variant(
        &mut self,
        literal: StructLiteral,
        target: ResolvedVariant,
        expected_ty: &Type,
    ) -> Expression {
        let StructLiteral {
            name: variant_name,
            fields: field_assignments,
            spread,
            span,
        } = literal;
        let ResolvedVariant {
            fields: variant_fields,
            substitutions: map,
            ty: enum_ty,
        } = target;
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

        if let StructSpread::Autofill { span: spread_span } = &new_spread {
            self.check_autofill_fields(
                &variant_name,
                variant_fields.iter().map(|f| (&f.name, &f.ty)),
                &matched_fields,
                &map,
                *spread_span,
            );
        }

        if let StructSpread::From(spread_expression) = &new_spread {
            self.check_enum_spread_fields(
                &resolved_enum,
                &variant_name,
                &variant_fields,
                &matched_fields,
                spread_expression.get_span(),
            );
        }

        if let Type::Nominal { id, .. } = &resolved_enum {
            let enum_id = id.as_str();
            self.register_construction_obligations(enum_id, &enum_ty, span);
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
            StructSpread::Autofill { span } => StructSpread::Autofill { span },
        }
    }

    fn check_enum_spread_fields(
        &mut self,
        resolved_enum: &Type,
        written_name: &str,
        variant_fields: &[syntax::ast::EnumFieldDefinition],
        matched_fields: &HashSet<EcoString>,
        spread_span: Span,
    ) {
        let store = self.store;
        let Type::Nominal { id, .. } = resolved_enum else {
            return;
        };
        let Some(variants) = store.variants_of(id) else {
            return;
        };
        let enum_name = unqualified_name(id);
        let target_variant = unqualified_name(written_name);
        let written_enum = written_name
            .rsplit_once('.')
            .map_or(enum_name, |(prefix, _)| prefix);
        let Some(target_index) = variants.iter().position(|v| v.name == target_variant) else {
            return;
        };
        let slots = syntax::go_names::enum_field_slots(enum_name, variants);
        let missing: Vec<String> = variant_fields
            .iter()
            .enumerate()
            .filter(|(_, field)| !matched_fields.contains(&field.name))
            .filter(|(field_index, field)| {
                let Some(target_slot) = slots[target_index].get(*field_index) else {
                    return false;
                };
                !variants.iter().enumerate().all(|(other_variant, variant)| {
                    variant
                        .fields
                        .iter()
                        .enumerate()
                        .any(|(other_index, other)| {
                            other.name == field.name
                                && slots[other_variant].get(other_index) == Some(target_slot)
                        })
                })
            })
            .map(|(_, field)| field.name.to_string())
            .collect();
        if missing.is_empty() {
            return;
        }
        let counterexample = missing.iter().find_map(|field_name| {
            variants
                .iter()
                .find(|variant| !variant.fields.iter().any(|f| f.name == field_name.as_str()))
                .map(|variant| (variant.name.as_str(), field_name.as_str()))
        });
        let builtin_collisions = missing
            .iter()
            .filter(|field_name| {
                syntax::go_names::is_builtin_enum_member(&syntax::go_names::snake_to_camel(
                    field_name,
                ))
            })
            .count();
        let reason = if builtin_collisions == missing.len() {
            diagnostics::infer::SeparateSlotReason::BuiltinMember
        } else if builtin_collisions == 0 {
            diagnostics::infer::SeparateSlotReason::ConflictingTypes
        } else {
            diagnostics::infer::SeparateSlotReason::Mixed
        };
        self.sink
            .push(diagnostics::infer::enum_spread_missing_fields(
                written_enum,
                target_variant,
                &missing,
                counterexample,
                reason,
                spread_span,
            ));
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

    fn check_autofill_fields<'a>(
        &mut self,
        owner_name: &str,
        fields: impl Iterator<Item = (&'a EcoString, &'a Type)>,
        matched_fields: &HashSet<EcoString>,
        map: &SubstitutionMap,
        spread_span: Span,
    ) {
        let from_package = self.cursor.package_id.clone();
        for (name, ty) in fields {
            if matched_fields.contains(name.as_str()) {
                continue;
            }
            let resolved = substitute(ty, map).resolve_in(&self.env);
            let Err(no_zero) = self.has_zero(&resolved, &from_package) else {
                continue;
            };
            let chain: Vec<&str> = no_zero.chain.iter().map(EcoString::as_str).collect();
            let cause = match &no_zero.reason {
                NoZeroReason::PrivateField {
                    struct_name,
                    field,
                    owning_package,
                } => diagnostics::infer::FieldNoZeroCause::PrivateField {
                    struct_name,
                    field,
                    owning_package,
                },
                NoZeroReason::HiddenGoState { go_type } => {
                    diagnostics::infer::FieldNoZeroCause::HiddenGoState { go_type }
                }
                NoZeroReason::NoZeroForType => diagnostics::infer::FieldNoZeroCause::Type,
            };
            self.sink.push(diagnostics::infer::field_no_zero(
                owner_name,
                name,
                &no_zero.leaf_ty,
                &chain,
                cause,
                spread_span,
            ));
        }
    }

    pub(crate) fn has_zero(&self, ty: &Type, from_package: &str) -> Result<(), NoZero> {
        let store = self.store;
        crate::zero::has_zero(store, ty, from_package)
    }
}

pub(super) fn same_nominal(a: &Type, b: &Type) -> bool {
    matches!(
        (a, b),
        (Type::Nominal { id: ai, .. }, Type::Nominal { id: bi, .. }) if ai == bi
    )
}
