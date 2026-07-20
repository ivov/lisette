use crate::checker::EnvResolve;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{
    Annotation, Attribute, EnumFieldDefinition, EnumVariant, Generic, Span, StructFieldDefinition,
    StructKind, VariantFields,
};
use syntax::containment::{EnumPayloads, definition_contains_by_value};
use syntax::program::{Attributes, Definition, DefinitionBody, MethodSignatures, Visibility};
use syntax::types::{Symbol, Type};

use super::enum_variant_constructor_type;
use crate::checker::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn populate_enum(
        &mut self,
        store: &mut Store,
        name: &str,
        name_span: &Span,
        generics: &[Generic],
        variants: &[EnumVariant],
        span: &Span,
        doc: &Option<String>,
        attributes: Attributes,
    ) {
        let qualified_name = self.qualify_name(name);
        let enum_ty = store
            .get_type(&qualified_name)
            .expect("enum type must exist")
            .clone();

        self.scopes.push();
        self.put_in_scope(generics);
        let generics = self.resolve_generic_bounds(&*store, generics, span);

        let new_variants: Vec<_> = variants
            .iter()
            .map(|v| self.resolve_enum_variant_fields(&*store, v, span))
            .collect();

        self.scopes.pop();

        self.check_enum_field_type_conflicts(name, &new_variants);

        for new_variant in &new_variants {
            self.add_enum_variant_to_scope(new_variant, name, &enum_ty, &generics);
        }

        let visibility = self
            .current_module(&*store)
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility.clone())
            .unwrap_or(Visibility::Private);

        let is_prelude = self.cursor.module_id == "prelude";

        let variant_definitions: Vec<_> = new_variants
            .iter()
            .map(|v| {
                let variant_ty = enum_variant_constructor_type(v, &enum_ty, &generics);
                let qualified_variant_name = qualified_name.with_segment(&v.name);
                let simple_qualified_name = if is_prelude {
                    Some(self.qualify_name(&v.name))
                } else {
                    None
                };
                (
                    qualified_variant_name,
                    simple_qualified_name,
                    variant_ty,
                    v.name_span,
                    v.doc.clone(),
                )
            })
            .collect();

        if self.is_lis(&*store) && self.type_definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "enum", name, *name_span,
            ));
        }

        let module = self.current_module_mut(store);

        for (qualified_variant_name, simple_name, variant_ty, variant_name_span, variant_doc) in
            variant_definitions
        {
            let definition = Definition {
                visibility: visibility.clone(),
                ty: variant_ty,
                name: None,
                name_span: Some(variant_name_span),
                doc: variant_doc,
                body: DefinitionBody::Value {
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                    const_value: None,
                },
            };
            module
                .definitions
                .insert(qualified_variant_name, definition.clone());

            if let Some(simple_qualified_name) = simple_name {
                module
                    .definitions
                    .entry(simple_qualified_name)
                    .or_insert(definition);
            }
        }

        module.definitions.insert(
            qualified_name.clone(),
            Definition {
                visibility,
                ty: enum_ty,
                name: Some(name.into()),
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Enum {
                    generics,
                    variants: new_variants,
                    methods: MethodSignatures::default(),
                    attributes,
                },
            },
        );
    }

    /// Check for Go-level field name collisions across enum variants.
    ///
    /// Computes each field's Go name via the shared authority in
    /// `syntax::go_names` and rejects same-name-different-type conflicts.
    fn check_enum_field_type_conflicts(&mut self, name: &str, variants: &[EnumVariant]) {
        if self.cursor.module_id == "prelude" {
            return;
        }

        // (variant_name, field_name, is_struct, type, span)
        let mut seen: FxHashMap<String, (&str, &str, bool, &Type, Span)> = FxHashMap::default();

        for variant in variants {
            let is_struct = variant.fields.is_struct();
            let single_field = variant.fields.len() == 1;

            for (fi, field) in variant.fields.iter().enumerate() {
                let go_name = syntax::go_names::enum_field_go_name(
                    &variant.name,
                    &field.name,
                    fi,
                    is_struct,
                    single_field,
                    name,
                );

                let resolved = field.ty.resolve_in(&self.env);
                let annotation_span = field.annotation.get_span();
                let span = if !annotation_span.is_dummy() {
                    annotation_span
                } else {
                    variant.name_span
                };
                let Some(&(v_a, f_a, is_struct_a, ty_a, _)) = seen.get(&go_name) else {
                    seen.insert(
                        go_name,
                        (&variant.name, &field.name, is_struct, &field.ty, span),
                    );
                    continue;
                };

                let ty_a_resolved = ty_a.resolve_in(&self.env);
                if matches!(ty_a_resolved, Type::Error)
                    || matches!(resolved, Type::Error)
                    || ty_a_resolved == resolved
                {
                    continue;
                }

                let loc_a = if is_struct_a {
                    format!("{}.{}.{}", name, v_a, f_a)
                } else {
                    format!("{}.{}", name, v_a)
                };
                let loc_b = if is_struct {
                    format!("{}.{}.{}", name, variant.name, field.name)
                } else {
                    format!("{}.{}", name, variant.name)
                };
                self.sink.push(diagnostics::infer::enum_field_type_conflict(
                    &loc_a,
                    &ty_a_resolved.to_string(),
                    &loc_b,
                    &resolved.to_string(),
                    span,
                ));
            }
        }
    }

    fn resolve_enum_variant_fields(
        &mut self,
        store: &Store,
        enum_variant: &EnumVariant,
        span: &Span,
    ) -> EnumVariant {
        let new_fields = match &enum_variant.fields {
            VariantFields::Unit => VariantFields::Unit,
            VariantFields::Tuple(fields) => {
                let resolved_fields = self.resolve_enum_fields(store, fields, span);
                VariantFields::Tuple(resolved_fields)
            }
            VariantFields::Struct(fields) => {
                let resolved_fields = self.resolve_enum_fields(store, fields, span);
                VariantFields::Struct(resolved_fields)
            }
        };

        EnumVariant {
            doc: enum_variant.doc.clone(),
            name: enum_variant.name.clone(),
            name_span: enum_variant.name_span,
            fields: new_fields,
        }
    }

    fn resolve_enum_fields(
        &mut self,
        store: &Store,
        fields: &[EnumFieldDefinition],
        span: &Span,
    ) -> Vec<EnumFieldDefinition> {
        fields
            .iter()
            .map(|f| {
                let resolved_ty = self.convert_to_type(store, &f.annotation, span);
                if let Type::Var { id, .. } = &f.ty {
                    self.env.bind(*id, resolved_ty.clone());
                }
                EnumFieldDefinition {
                    ty: resolved_ty,
                    ..f.clone()
                }
            })
            .collect()
    }

    pub(crate) fn add_enum_variant_to_scope(
        &mut self,
        variant: &EnumVariant,
        enum_name: &str,
        enum_ty: &Type,
        generics: &[Generic],
    ) {
        let enum_variant_constructor_ty = enum_variant_constructor_type(variant, enum_ty, generics);
        let qualified_name = format!("{}.{}", enum_name, variant.name);

        let scope = self.scopes.current_mut();

        scope
            .values
            .insert(qualified_name.clone(), enum_variant_constructor_ty.clone());

        scope
            .values
            .entry(variant.name.to_string())
            .or_insert(enum_variant_constructor_ty);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn populate_struct(
        &mut self,
        store: &mut Store,
        name: &str,
        name_span: &Span,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
        kind: StructKind,
        span: &Span,
        doc: &Option<String>,
        attributes: Attributes,
    ) {
        let qualified_name = self.qualify_name(name);
        let struct_ty = store
            .get_type(&qualified_name)
            .expect("struct type scheme must exist")
            .clone();

        self.scopes.push();
        self.put_in_scope(generics);
        let generics = self.resolve_generic_bounds(&*store, generics, span);

        let new_fields: Vec<StructFieldDefinition> = fields
            .iter()
            .map(|f| {
                let field_ty = self.convert_to_type(&*store, &f.annotation, span);
                let visibility = if f.embedded {
                    embed_field_visibility(&*store, &field_ty)
                } else {
                    f.visibility
                };
                StructFieldDefinition {
                    ty: field_ty,
                    visibility,
                    ..f.clone()
                }
            })
            .collect();

        self.scopes.pop();

        // Single-field non-generic tuple structs (e.g. `struct FileMode(uint32)`) are
        // emitted as Go type aliases (`type FileMode uint32`). Set underlying_ty so the
        // type checker allows numeric casts through them.
        let struct_ty = if kind == StructKind::Tuple && new_fields.len() == 1 && generics.is_empty()
        {
            match struct_ty {
                Type::Nominal { id, params, .. } => Type::Nominal {
                    id,
                    params,
                    underlying_ty: Some(Box::new(new_fields[0].ty.clone())),
                },
                other => other,
            }
        } else {
            struct_ty
        };

        let visibility = self
            .current_module(&*store)
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility.clone())
            .unwrap_or(Visibility::Private);

        if self.is_lis(&*store) && self.type_definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "struct", name, *name_span,
            ));
        }

        self.current_module_mut(store).definitions.insert(
            qualified_name.clone(),
            Definition {
                visibility,
                ty: struct_ty,
                name: Some(name.into()),
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Struct {
                    generics,
                    fields: new_fields,
                    kind,
                    methods: Default::default(),
                    constructor: None,
                    attributes,
                },
            },
        );
    }

    pub(super) fn validate_module_embeds(&mut self, store: &Store, module_id: &str) {
        let Some(module) = store.get_module(module_id) else {
            return;
        };
        for definition in module.definitions.values() {
            if definition
                .name_span
                .is_some_and(|span| module.typedefs.contains_key(&span.file_id))
            {
                continue;
            }
            let DefinitionBody::Struct { fields, .. } = &definition.body else {
                continue;
            };
            for field in fields.iter().filter(|f| f.embedded) {
                self.validate_embed_target(store, &field.ty, field.name_span);
            }
        }
    }

    fn validate_embed_target(&mut self, store: &Store, ty: &Type, span: Span) {
        let display = ty.to_string();
        let resolved = store.deep_resolve_alias(ty);

        if resolved.is_option() {
            self.sink.push(diagnostics::embed::option_target(span));
            return;
        }

        let promotion_target = embed_promotion_target(store, &resolved);
        if is_imported_nominal(&promotion_target)
            && !is_faithful_imported_embed_target(store, &promotion_target)
        {
            self.sink
                .push(diagnostics::embed::imported_target(&display, span));
            return;
        }

        if resolved.is_ref() {
            match resolved
                .inner()
                .map(|inner| store.deep_resolve_alias(&inner))
            {
                Some(inner) if inner.is_ref() => {
                    self.sink
                        .push(diagnostics::embed::nested_ref(&display, span));
                }
                Some(inner) if store.is_interface(&inner) => {
                    self.sink
                        .push(diagnostics::embed::pointer_to_interface(&display, span));
                }
                Some(inner) if is_pointer_backed_newtype(store, &inner) => {
                    self.sink
                        .push(diagnostics::embed::pointer_backed_newtype(&display, span));
                }
                Some(inner) if is_deferred_local_target(store, &inner) => {
                    self.sink
                        .push(diagnostics::embed::defined_type(&display, span));
                }
                Some(inner)
                    if is_embeddable_nominal(&inner) && has_selector_surface(store, &inner) => {}
                _ => self
                    .sink
                    .push(diagnostics::embed::no_surface(&display, span)),
            }
            return;
        }

        if is_pointer_backed_newtype(store, &resolved) {
            self.sink
                .push(diagnostics::embed::pointer_backed_newtype(&display, span));
            return;
        }

        if is_deferred_local_target(store, &resolved) {
            self.sink
                .push(diagnostics::embed::defined_type(&display, span));
            return;
        }

        if !is_embeddable_nominal(&resolved) || !has_selector_surface(store, &resolved) {
            self.sink
                .push(diagnostics::embed::no_surface(&display, span));
        }
    }

    pub(super) fn check_module_recursive_types(&mut self, store: &Store, module_id: &str) {
        if module_id.starts_with("go:") {
            return;
        }
        let Some(module) = store.get_module(module_id) else {
            return;
        };

        let mut targets: Vec<(&str, &str, Span)> = module
            .definitions
            .iter()
            .filter(|(_, definition)| matches!(definition.body, DefinitionBody::Struct { .. }))
            .filter_map(|(qualified_name, definition)| {
                let span = definition.name_span?;
                if module.typedefs.contains_key(&span.file_id) {
                    return None;
                }
                Some((qualified_name.as_str(), definition.name.as_deref()?, span))
            })
            .collect();
        targets.sort_by_key(|(_, _, span)| (span.file_id, span.byte_offset));

        let mut flagged: FxHashSet<String> = FxHashSet::default();
        for (qualified_name, name, span) in targets {
            if definition_contains_by_value(
                qualified_name,
                qualified_name,
                EnumPayloads::Skip,
                &flagged,
                |id| store.get_definition(id),
            ) {
                self.sink
                    .push(diagnostics::infer::recursive_type(name, span));
                flagged.insert(qualified_name.to_string());
            }
        }
    }

    pub(super) fn settle_module_aliases(&self, store: &mut Store, module_id: &str) {
        if module_id.starts_with("go:") {
            return;
        }
        let Some(module) = store.get_module(module_id) else {
            return;
        };

        let updates: Vec<(Symbol, Type)> = module
            .definitions
            .iter()
            .filter(|(_, definition)| matches!(definition.body, DefinitionBody::TypeAlias { .. }))
            .filter_map(|(name, definition)| {
                let mut changed = false;
                let mut in_progress = FxHashSet::default();
                let filled =
                    fill_alias_underlyings(&definition.ty, store, &mut changed, &mut in_progress);
                changed.then(|| (name.clone(), filled))
            })
            .collect();

        let Some(module) = store.get_module_mut(module_id) else {
            return;
        };
        for (name, ty) in updates {
            if let Some(definition) = module.definitions.get_mut(&name) {
                definition.ty = ty;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn populate_type_alias(
        &mut self,
        store: &mut Store,
        name: &str,
        name_span: &Span,
        generics: &[Generic],
        annotation: &Annotation,
        attributes: &[Attribute],
        span: &Span,
        doc: &Option<String>,
    ) {
        let qualified_name = self.qualify_name(name);

        self.scopes.push();
        self.put_in_scope(generics);
        let generics = self.resolve_generic_bounds(&*store, generics, span);

        if annotation.is_opaque() {
            if self.is_lis(&*store) {
                self.sink
                    .push(diagnostics::infer::opaque_type_outside_typedef(*span));
            }

            let visibility = self
                .current_module(&*store)
                .definitions
                .get(qualified_name.as_str())
                .map(|definition| definition.visibility.clone())
                .unwrap_or(Visibility::Private);

            let alias_ty = if name == "Never" && generics.is_empty() {
                Type::Never
            } else {
                let params: Vec<Type> = generics
                    .iter()
                    .map(|g| Type::Parameter(g.name.clone()))
                    .collect();

                let canonical_ty = if self.cursor.module_id == "prelude" {
                    if let Some(simple) = syntax::types::SimpleKind::from_name(name) {
                        Type::Simple(simple)
                    } else if let Some(compound) = syntax::types::CompoundKind::from_name(name) {
                        Type::Compound {
                            kind: compound,
                            args: params,
                        }
                    } else {
                        Type::Nominal {
                            id: qualified_name.clone(),
                            params,
                            underlying_ty: None,
                        }
                    }
                } else {
                    Type::Nominal {
                        id: qualified_name.clone(),
                        params,
                        underlying_ty: None,
                    }
                };

                if generics.is_empty() {
                    canonical_ty
                } else {
                    Type::Forall {
                        vars: generics.iter().map(|g| g.name.clone()).collect(),
                        body: Box::new(canonical_ty),
                    }
                }
            };

            if self.is_lis(&*store) && self.type_definition_exists(&*store, &qualified_name) {
                self.sink.push(diagnostics::infer::duplicate_definition(
                    "type alias",
                    name,
                    *name_span,
                ));
            }

            self.scopes.pop();

            self.current_module_mut(store).definitions.insert(
                qualified_name,
                Definition {
                    visibility,
                    ty: alias_ty,
                    name: Some(name.into()),
                    name_span: Some(*name_span),
                    doc: doc.clone(),
                    body: DefinitionBody::TypeAlias {
                        generics,
                        annotation: annotation.clone(),
                        methods: Default::default(),
                        attributes: super::collect_struct_attributes(attributes),
                    },
                },
            );

            return;
        }

        let body_ty = self.convert_to_type(&*store, annotation, span);
        let is_function_body = matches!(body_ty, Type::Function(_));

        if !is_function_body && self.is_alias_body_circular(&*store, &body_ty, &qualified_name) {
            self.sink
                .push(diagnostics::infer::circular_type_alias(name, *span));
        }

        let body_ty = if is_function_body {
            let params: Vec<Type> = generics
                .iter()
                .map(|g| Type::Parameter(g.name.clone()))
                .collect();
            Type::Nominal {
                id: qualified_name.clone(),
                params,
                underlying_ty: Some(Box::new(body_ty)),
            }
        } else {
            body_ty
        };

        let alias_ty = if generics.is_empty() {
            body_ty
        } else {
            Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(body_ty),
            }
        };

        self.scopes.pop();

        let visibility = self
            .current_module(&*store)
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility.clone())
            .unwrap_or(Visibility::Private);

        if self.is_lis(&*store) && self.type_definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "type alias",
                name,
                *name_span,
            ));
        }

        self.current_module_mut(store).definitions.insert(
            qualified_name,
            Definition {
                visibility,
                ty: alias_ty,
                name: Some(name.into()),
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::TypeAlias {
                    generics,
                    annotation: annotation.clone(),
                    methods: Default::default(),
                    attributes: super::collect_struct_attributes(attributes),
                },
            },
        );
    }

    fn is_alias_body_circular(&self, store: &Store, body_ty: &Type, qualified_name: &str) -> bool {
        if Self::type_contains_name(body_ty, qualified_name) {
            return true;
        }

        let mut to_visit: Vec<String> = Vec::new();
        Self::collect_type_refs(body_ty, &mut to_visit);

        let mut seen: Vec<String> = Vec::new();
        while let Some(name) = to_visit.pop() {
            if name == qualified_name {
                return true;
            }
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());

            if let Some(def) = store.get_definition(&name)
                && matches!(def.body, DefinitionBody::TypeAlias { .. })
            {
                let body = def.ty.unwrap_forall().clone();
                if Self::type_contains_name(&body, qualified_name) {
                    return true;
                }
                Self::collect_type_refs(&body, &mut to_visit);
            }
        }

        false
    }

    fn type_contains_name(ty: &Type, name: &str) -> bool {
        if let Type::Nominal { id, .. } = ty
            && id.as_str() == name
        {
            return true;
        }
        ty.children()
            .iter()
            .any(|c| Self::type_contains_name(c, name))
    }

    fn collect_type_refs(ty: &Type, refs: &mut Vec<String>) {
        if let Type::Nominal { id, .. } = ty {
            refs.push(id.to_string());
        }
        for c in ty.children() {
            Self::collect_type_refs(c, refs);
        }
    }
}

fn is_embeddable_nominal(ty: &Type) -> bool {
    matches!(ty, Type::Nominal { .. }) && !ty.is_option()
}

fn is_imported_nominal(ty: &Type) -> bool {
    matches!(ty, Type::Nominal { id, .. } if id.as_str().starts_with(syntax::types::GO_IMPORT_PREFIX))
}

fn is_faithful_imported_embed_target(store: &Store, ty: &Type) -> bool {
    is_faithful_imported_graph(store, ty, &mut rustc_hash::FxHashSet::default())
}

fn is_faithful_imported_graph(
    store: &Store,
    ty: &Type,
    seen: &mut rustc_hash::FxHashSet<String>,
) -> bool {
    let target = store.deep_resolve_alias(&embed_promotion_target(store, ty));
    let Type::Nominal { id, .. } = &target else {
        return false;
    };
    if !id.as_str().starts_with(syntax::types::GO_IMPORT_PREFIX) {
        return false;
    }
    if !seen.insert(id.to_string()) {
        return true;
    }
    let Some(definition) = store.get_definition(id.as_str()) else {
        return false;
    };
    // `#[go(hidden_embed)]` looks flat but hides an embed bindgen could not emit, so it is not faithful.
    if definition.has_hidden_embed() {
        return false;
    }
    match &definition.body {
        DefinitionBody::Struct {
            fields,
            kind: StructKind::Record,
            generics,
            ..
        } if generics.is_empty() => fields
            .iter()
            .filter(|field| field.embedded)
            .all(|field| is_faithful_imported_graph(store, &field.ty, seen)),
        DefinitionBody::Struct { generics, .. } if generics.is_empty() => {
            has_selector_surface(store, &target)
        }
        DefinitionBody::Interface { .. } => has_selector_surface(store, &target),
        DefinitionBody::TypeAlias { .. } => has_selector_surface(store, &target),
        _ => false,
    }
}

fn embed_promotion_target(store: &Store, ty: &Type) -> Type {
    let mut target = ty.clone();
    while target.is_option() || target.is_ref() {
        let Some(inner) = target.inner() else { break };
        target = store.deep_resolve_alias(&inner);
    }
    target
}

// A local tuple struct, newtype, or enum: needs the resolver, so hard-errored for now.
fn is_deferred_local_target(store: &Store, ty: &Type) -> bool {
    let Type::Nominal { id, .. } = ty else {
        return false;
    };
    let id = id.as_str();
    if id.starts_with(syntax::types::GO_IMPORT_PREFIX) {
        return false;
    }
    match store.get_definition(id).map(|definition| &definition.body) {
        Some(
            DefinitionBody::Struct {
                kind: StructKind::Record,
                ..
            }
            | DefinitionBody::Interface { .. },
        ) => false,
        Some(_) => true,
        None => false,
    }
}

fn has_selector_surface(store: &Store, ty: &Type) -> bool {
    if !store
        .get_all_methods(ty, &rustc_hash::FxHashMap::default())
        .is_empty()
    {
        return true;
    }
    let Type::Nominal { id, .. } = ty else {
        return false;
    };
    let id = id.as_str();
    let is_newtype = store.get_definition(id).is_some_and(Definition::is_newtype);
    !is_newtype && store.fields_of(id).is_some_and(|fields| !fields.is_empty())
}

fn is_pointer_backed_newtype(store: &Store, ty: &Type) -> bool {
    let Type::Nominal { id, .. } = ty else {
        return false;
    };
    store
        .get_type(id.as_str())
        .and_then(Type::get_underlying)
        .is_some_and(|underlying| store.deep_resolve_alias(underlying).is_ref())
}

fn fill_alias_underlyings(
    ty: &Type,
    store: &Store,
    changed: &mut bool,
    in_progress: &mut FxHashSet<Symbol>,
) -> Type {
    match ty {
        Type::Nominal {
            id,
            params,
            underlying_ty,
        } => {
            let params: Vec<Type> = params
                .iter()
                .map(|param| fill_alias_underlyings(param, store, changed, in_progress))
                .collect();
            let underlying = match underlying_ty {
                Some(inner) => Some(Box::new(fill_alias_underlyings(
                    inner,
                    store,
                    changed,
                    in_progress,
                ))),
                None if in_progress.contains(id) => None,
                None => {
                    let probe = Type::Nominal {
                        id: id.clone(),
                        params: params.clone(),
                        underlying_ty: None,
                    };
                    let resolved = store.deep_resolve_alias(&probe);
                    if resolved.get_qualified_id() == Some(id.as_str()) {
                        None
                    } else {
                        *changed = true;
                        in_progress.insert(id.clone());
                        let filled = fill_alias_underlyings(&resolved, store, changed, in_progress);
                        in_progress.remove(id);
                        Some(Box::new(filled))
                    }
                }
            };
            Type::Nominal {
                id: id.clone(),
                params,
                underlying_ty: underlying,
            }
        }
        Type::Compound { kind, args } => Type::Compound {
            kind: *kind,
            args: args
                .iter()
                .map(|arg| fill_alias_underlyings(arg, store, changed, in_progress))
                .collect(),
        },
        Type::Array { length, element } => Type::Array {
            length: *length,
            element: Box::new(fill_alias_underlyings(element, store, changed, in_progress)),
        },
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| fill_alias_underlyings(element, store, changed, in_progress))
                .collect(),
        ),
        Type::Forall { vars, body } => Type::Forall {
            vars: vars.clone(),
            body: Box::new(fill_alias_underlyings(body, store, changed, in_progress)),
        },
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| fill_alias_underlyings(param, store, changed, in_progress))
                .collect();
            let return_type = Box::new(fill_alias_underlyings(
                &function.return_type,
                store,
                changed,
                in_progress,
            ));
            function.rebuild(params, function.bounds.clone(), return_type)
        }
        other => other.clone(),
    }
}

// Mirror the written type's own visibility: peel storage (`Option`/`Ref`), not aliases.
fn embed_field_visibility(store: &Store, field_ty: &Type) -> syntax::ast::Visibility {
    let mut target = field_ty.clone();
    while target.is_option() || target.is_ref() {
        let Some(inner) = target.inner() else { break };
        target = inner;
    }
    let public = matches!(&target, Type::Nominal { id, .. }
        if store.get_definition(id.as_str()).is_some_and(|d| d.visibility.is_public()));
    if public {
        syntax::ast::Visibility::Public
    } else {
        syntax::ast::Visibility::Private
    }
}
