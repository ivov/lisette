use rustc_hash::FxHashSet as HashSet;

use crate::checker::EnvResolve;
use crate::store::Store;
use ecow::EcoString;
use syntax::ast::{Expression, Span, StructFieldAssignment, StructSpread};
use syntax::program::Definition;
use syntax::types::{CompoundKind, SimpleKind, SubstitutionMap, Symbol, Type, substitute};

use super::super::TaskState;

/// Chain of field accesses leading to a non-zero-constructible field.
/// Used to render diagnostics like "outer.inner.b is private to module other".
#[derive(Debug, Clone)]
pub(crate) struct NoZero {
    pub(crate) chain: Vec<EcoString>,
    pub(crate) reason: NoZeroReason,
    pub(crate) leaf_ty: Type,
}

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

#[derive(Debug, Clone)]
pub(crate) enum NoZeroReason {
    /// The leaf type itself has no defined zero (e.g., bare `fn`, `Channel<T>`,
    /// `Ref<T>`, `Result<T, E>`, enum without default variant).
    NoZeroForType,
    /// A nested user-defined struct has a private field unreachable from the
    /// calling module.
    PrivateField {
        struct_name: EcoString,
        field: EcoString,
        owning_module: EcoString,
    },
}

impl TaskState<'_> {
    pub(super) fn infer_struct_call(
        &mut self,
        store: &mut Store,
        struct_name: EcoString,
        field_assignments: Vec<StructFieldAssignment>,
        spread: StructSpread,
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
        spread: StructSpread,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let (struct_call_ty, map) = self.instantiate(&struct_ty);

        let peeled_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        if same_nominal(&peeled_expected, &struct_call_ty) && !peeled_expected.contains_unknown() {
            let _ = self.speculatively(|this| {
                this.try_unify(store, &peeled_expected, &struct_call_ty, &span)
            });
        }

        let new_spread = self.infer_struct_spread(store, spread, &struct_call_ty);

        let struct_module = qualified_name.split('.').next().unwrap_or(&qualified_name);
        let is_cross_module = struct_module != self.cursor.module_id
            || struct_name
                .split_once('.')
                .is_some_and(|(prefix, _)| self.imports.imported_modules.contains_key(prefix));
        let is_go_imported = qualified_name.starts_with("go:");

        let (new_field_assignments, matched_fields) = self.infer_structish_fields(
            store,
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
                store,
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
            let owning_module = qualified_name
                .split_once('.')
                .map(|(m, _)| m)
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
        self.unify(store, &final_expected, &struct_call_ty, &span);

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
        store: &mut Store,
        variant_name: EcoString,
        variant_fields: Vec<syntax::ast::EnumFieldDefinition>,
        map: SubstitutionMap,
        field_assignments: Vec<StructFieldAssignment>,
        spread: StructSpread,
        span: Span,
        expected_ty: &Type,
        enum_ty: Type,
    ) -> Expression {
        self.unify(store, expected_ty, &enum_ty, &span);
        let new_spread = self.infer_struct_spread(store, spread, &enum_ty);

        let (new_field_assignments, matched_fields) = self.infer_structish_fields(
            store,
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
                store,
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

    fn infer_struct_spread(
        &mut self,
        store: &mut Store,
        spread: StructSpread,
        target_ty: &Type,
    ) -> StructSpread {
        match spread {
            StructSpread::None => StructSpread::None,
            StructSpread::From(s) => {
                let inferred = self
                    .with_value_context(|checker| checker.infer_expression(store, *s, target_ty));
                StructSpread::From(Box::new(inferred))
            }
            StructSpread::ZeroFill { span } => StructSpread::ZeroFill { span },
        }
    }

    fn infer_structish_fields<'a, FindDef>(
        &mut self,
        store: &mut Store,
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
        store: &Store,
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
            let Err(no_zero) = self.has_zero(store, &resolved, &from_module) else {
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
    pub(crate) fn has_zero(
        &self,
        store: &Store,
        ty: &Type,
        from_module: &str,
    ) -> Result<(), NoZero> {
        has_zero(store, ty, from_module)
    }
}

/// Predicate: does `ty` have a Lisette-side zero, constructible from `from_module`?
/// Returns `Err(NoZero)` with a chain of field accesses to the offending leaf when
/// no zero is available; `Ok(())` otherwise.
#[allow(clippy::result_large_err)]
pub(crate) fn has_zero(store: &Store, ty: &Type, from_module: &str) -> Result<(), NoZero> {
    match ty {
        Type::Simple(kind) => match kind {
            SimpleKind::Bool
            | SimpleKind::String
            | SimpleKind::Int
            | SimpleKind::Int8
            | SimpleKind::Int16
            | SimpleKind::Int32
            | SimpleKind::Int64
            | SimpleKind::Uint
            | SimpleKind::Uint8
            | SimpleKind::Uint16
            | SimpleKind::Uint32
            | SimpleKind::Uint64
            | SimpleKind::Uintptr
            | SimpleKind::Byte
            | SimpleKind::Float32
            | SimpleKind::Float64
            | SimpleKind::Complex64
            | SimpleKind::Complex128
            | SimpleKind::Rune
            | SimpleKind::Unit => Ok(()),
        },
        Type::Compound { kind, .. } => match kind {
            // Slice<T>, Map<K,V> always have a zero (empty, non-nil).
            CompoundKind::Slice | CompoundKind::Map | CompoundKind::EnumeratedSlice => Ok(()),
            // Ref<T>, Channel<T>, Sender<T>, Receiver<T>, VarArgs<T> have no zero.
            CompoundKind::Ref
            | CompoundKind::Channel
            | CompoundKind::Sender
            | CompoundKind::Receiver
            | CompoundKind::VarArgs => Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: ty.clone(),
            }),
        },
        Type::Tuple(elements) => {
            for (i, e) in elements.iter().enumerate() {
                if let Err(mut nz) = has_zero(store, e, from_module) {
                    let mut chain = vec![EcoString::from(i.to_string())];
                    chain.append(&mut nz.chain);
                    nz.chain = chain;
                    return Err(nz);
                }
            }
            Ok(())
        }
        Type::Function { .. } => Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: ty.clone(),
        }),
        Type::Nominal { id, params, .. } => {
            if id.as_str() == "prelude.Option" {
                // Option<T>'s zero is None regardless of T. Stop recursion.
                return Ok(());
            }
            has_zero_nominal(store, id, params, from_module, ty)
        }
        Type::Forall { body, .. } => has_zero(store, body, from_module),
        Type::Var { .. } | Type::Parameter(_) | Type::ReceiverPlaceholder => {
            // Conservative: unresolved/abstract types have no known zero.
            Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: ty.clone(),
            })
        }
        Type::Never | Type::Error | Type::ImportNamespace(_) => Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: ty.clone(),
        }),
    }
}

#[allow(clippy::result_large_err)]
fn has_zero_nominal(
    store: &Store,
    id: &Symbol,
    params: &[Type],
    from_module: &str,
    original_ty: &Type,
) -> Result<(), NoZero> {
    // Go-imported nominal: every Go field has a Go zero by language definition.
    // Accept the whole nominal without recursing into its fields (Go's own
    // `T{}` zeroing is what the emit will use).
    if id.as_str().starts_with("go:") {
        return Ok(());
    }

    let Some(def) = store.get_definition(id.as_str()) else {
        // Unknown nominal — conservatively reject.
        return Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: original_ty.clone(),
        });
    };

    match def {
        Definition::Struct {
            fields, ty: def_ty, ..
        } => {
            let map = build_substitution(def_ty, params);
            let struct_module = id
                .as_str()
                .split_once('.')
                .map(|(m, _)| m)
                .unwrap_or(from_module);
            let struct_is_foreign = struct_module != from_module;
            let struct_name: EcoString = id.last_segment().into();
            for f in fields {
                if struct_is_foreign && !f.visibility.is_public() {
                    return Err(NoZero {
                        chain: vec![f.name.clone()],
                        reason: NoZeroReason::PrivateField {
                            struct_name: struct_name.clone(),
                            field: f.name.clone(),
                            owning_module: EcoString::from(struct_module),
                        },
                        leaf_ty: f.ty.clone(),
                    });
                }
                let resolved = if map.is_empty() {
                    f.ty.clone()
                } else {
                    substitute(&f.ty, &map)
                };
                if let Err(mut nz) = has_zero(store, &resolved, from_module) {
                    let mut chain = vec![f.name.clone()];
                    chain.append(&mut nz.chain);
                    nz.chain = chain;
                    return Err(nz);
                }
            }
            Ok(())
        }
        Definition::TypeAlias {
            ty: alias_ty,
            annotation,
            ..
        } => {
            if annotation.is_opaque() {
                return Err(NoZero {
                    chain: vec![],
                    reason: NoZeroReason::NoZeroForType,
                    leaf_ty: original_ty.clone(),
                });
            }
            let underlying = match alias_ty {
                Type::Forall { body, .. } => body.as_ref().clone(),
                other => other.clone(),
            };
            let map = build_substitution(alias_ty, params);
            let resolved = if map.is_empty() {
                underlying
            } else {
                substitute(&underlying, &map)
            };
            has_zero(store, &resolved, from_module)
        }
        // Enums and other definitions have no zero unless we add a designated
        // default-variant mechanism later.
        _ => Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: original_ty.clone(),
        }),
    }
}

fn build_substitution(def_ty: &Type, params: &[Type]) -> SubstitutionMap {
    let mut map = SubstitutionMap::default();
    if let Type::Forall { vars, .. } = def_ty
        && vars.len() == params.len()
    {
        for (var, param) in vars.iter().zip(params.iter()) {
            map.insert(var.clone(), param.clone());
        }
    }
    map
}

fn same_nominal(a: &Type, b: &Type) -> bool {
    matches!(
        (a, b),
        (Type::Nominal { id: ai, .. }, Type::Nominal { id: bi, .. }) if ai == bi
    )
}
