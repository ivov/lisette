use crate::checker::EnvResolve;
use syntax::ast::{
    Annotation, EnumFieldDefinition, EnumVariant, Generic, Span, StructFieldDefinition, StructKind,
    ValueEnumVariant, VariantFields,
};
use syntax::program::{Definition, MethodSignatures, Visibility};
use syntax::types::Type;

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
    ) {
        let qualified_name = self.qualify_name(name);
        let enum_ty = store
            .get_type(&qualified_name)
            .expect("enum type must exist")
            .clone();

        self.scopes.push();
        self.put_in_scope(generics);
        self.validate_generic_bounds(&*store, generics, span);
        self.scopes.pop();

        let new_variants: Vec<_> = variants
            .iter()
            .map(|v| self.resolve_enum_variant_fields(&*store, v, generics, span))
            .collect();

        self.check_enum_field_type_conflicts(name, &new_variants);

        for new_variant in &new_variants {
            self.add_enum_variant_to_scope(new_variant, name, &enum_ty, generics);
        }

        let visibility = store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility().clone())
            .unwrap_or(Visibility::Private);

        let is_prelude = self.cursor.module_id == "prelude";

        let variant_definitions: Vec<_> = new_variants
            .iter()
            .map(|v| {
                let variant_ty = enum_variant_constructor_type(v, &enum_ty, generics);
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

        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");

        for (qualified_variant_name, simple_name, variant_ty, variant_name_span, variant_doc) in
            variant_definitions
        {
            let definition = Definition::Value {
                visibility: visibility.clone(),
                ty: variant_ty,
                name_span: Some(variant_name_span),
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                doc: variant_doc,
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
            Definition::Enum {
                visibility,
                ty: enum_ty,
                name: name.into(),
                name_span: *name_span,
                generics: generics.to_vec(),
                variants: new_variants,
                methods: MethodSignatures::default(),
                doc: doc.clone(),
            },
        );

        self.check_recursive_type(store, &qualified_name, name, name_span);
    }

    pub(super) fn populate_value_enum(
        &mut self,
        store: &mut Store,
        name: &str,
        name_span: &Span,
        underlying_ty: Option<&Annotation>,
        variants: &[ValueEnumVariant],
        doc: &Option<String>,
    ) {
        if !self.is_d_lis(&*store) {
            let span = variants
                .first()
                .map(|v| v.value_span)
                .unwrap_or_else(|| *name_span);
            self.sink
                .push(diagnostics::infer::value_enum_in_source_file(name, span));
            return;
        }

        let qualified_name = self.qualify_name(name);
        let base_enum_ty = store
            .get_type(&qualified_name)
            .expect("enum type must exist")
            .clone();

        let visibility = store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility().clone())
            .unwrap_or(Visibility::Private);

        let underlying_ty =
            underlying_ty.map(|annotation| self.convert_to_type(&*store, annotation, name_span));

        let enum_ty = if let (Type::Nominal { id, params, .. }, Some(underlying)) =
            (&base_enum_ty, &underlying_ty)
        {
            Type::Nominal {
                id: id.clone(),
                params: params.clone(),
                underlying_ty: Some(Box::new(underlying.clone())),
            }
        } else {
            base_enum_ty
        };

        for variant in variants {
            let qualified_variant_name = qualified_name.with_segment(&variant.name);
            let module = store
                .get_module_mut(&self.cursor.module_id)
                .expect("current module must exist in store");
            module.definitions.insert(
                qualified_variant_name,
                Definition::Value {
                    visibility: visibility.clone(),
                    ty: enum_ty.clone(),
                    name_span: Some(variant.name_span),
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    doc: variant.doc.clone(),
                },
            );
        }

        let scope = self.scopes.current_mut();
        for variant in variants {
            let qualified_variant_name = format!("{}.{}", name, variant.name);
            scope.values.insert(qualified_variant_name, enum_ty.clone());
            scope
                .values
                .insert(variant.name.to_string(), enum_ty.clone());
        }

        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");

        module.definitions.insert(
            qualified_name,
            Definition::ValueEnum {
                visibility,
                ty: enum_ty,
                name: name.into(),
                name_span: *name_span,
                variants: variants.to_vec(),
                underlying_ty,
                methods: Default::default(),
                doc: doc.clone(),
            },
        );
    }

    /// Check for Go-level field name collisions across enum variants.
    ///
    /// Computes the Go field name for every variant field (struct fields get
    /// capitalized, single-tuple fields use the variant name, multi-tuple fields
    /// use variant name + index) and rejects same-name-different-type conflicts.
    fn check_enum_field_type_conflicts(&mut self, name: &str, variants: &[EnumVariant]) {
        if self.cursor.module_id == "prelude" {
            return;
        }

        // (variant_name, field_name, is_struct, type, span)
        let mut seen: rustc_hash::FxHashMap<String, (&str, &str, bool, &Type, Span)> =
            rustc_hash::FxHashMap::default();

        for variant in variants {
            let is_struct = variant.fields.is_struct();
            let single_field = variant.fields.len() == 1;

            for (fi, field) in variant.fields.iter().enumerate() {
                // Mirror the Go field name logic from enum_layout.rs
                let go_name = if is_struct {
                    let mut chars = field.name.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                } else if single_field {
                    variant.name.to_string()
                } else {
                    format!("{}{}", variant.name, fi)
                };

                let resolved = field.ty.resolve_in(&self.env);
                let annotation_span = field.annotation.get_span();
                let span = if !annotation_span.is_dummy() {
                    annotation_span
                } else {
                    variant.name_span
                };
                if let Some(&(v_a, f_a, is_struct_a, ty_a, _)) = seen.get(&go_name) {
                    if ty_a.resolve_in(&self.env) != resolved {
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
                            &ty_a.resolve_in(&self.env).to_string(),
                            &loc_b,
                            &resolved.to_string(),
                            span,
                        ));
                    }
                } else {
                    seen.insert(
                        go_name,
                        (&variant.name, &field.name, is_struct, &field.ty, span),
                    );
                }
            }
        }
    }

    fn resolve_enum_variant_fields(
        &mut self,
        store: &Store,
        enum_variant: &EnumVariant,
        enum_generics: &[Generic],
        span: &Span,
    ) -> EnumVariant {
        let new_fields = match &enum_variant.fields {
            VariantFields::Unit => VariantFields::Unit,
            VariantFields::Tuple(fields) => {
                let resolved_fields = self.resolve_enum_fields(store, fields, enum_generics, span);
                VariantFields::Tuple(resolved_fields)
            }
            VariantFields::Struct(fields) => {
                let resolved_fields = self.resolve_enum_fields(store, fields, enum_generics, span);
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
        enum_generics: &[Generic],
        span: &Span,
    ) -> Vec<EnumFieldDefinition> {
        self.scopes.push();
        self.put_in_scope(enum_generics);

        let resolved_fields = fields
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
            .collect();

        self.scopes.pop();

        resolved_fields
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
    ) {
        let qualified_name = self.qualify_name(name);
        let struct_ty = store
            .get_type(&qualified_name)
            .expect("struct type scheme must exist")
            .clone();

        self.scopes.push();
        self.put_in_scope(generics);
        self.validate_generic_bounds(&*store, generics, span);

        let new_fields: Vec<StructFieldDefinition> = fields
            .iter()
            .map(|f| {
                let field_ty = self.convert_to_type(&*store, &f.annotation, span);
                StructFieldDefinition {
                    ty: field_ty,
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

        let visibility = store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility().clone())
            .unwrap_or(Visibility::Private);

        if self.is_lis(&*store) && self.type_definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "struct", name, *name_span,
            ));
        }

        store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .insert(
                qualified_name.clone(),
                Definition::Struct {
                    visibility,
                    ty: struct_ty,
                    name: name.into(),
                    name_span: *name_span,
                    generics: generics.to_vec(),
                    fields: new_fields,
                    kind,
                    methods: Default::default(),
                    constructor: None,
                    doc: doc.clone(),
                },
            );

        self.check_recursive_type(&*store, &qualified_name, name, name_span);
    }

    /// Check whether a type is recursive without Ref indirection.
    /// A type that contains itself (directly or through Option, Tuple, etc.) without going
    /// through Ref has infinite size and is rejected by Go.
    fn check_recursive_type(
        &mut self,
        store: &Store,
        qualified_name: &str,
        struct_name: &str,
        name_span: &Span,
    ) {
        if self.contains_type_without_ref(
            store,
            qualified_name,
            qualified_name,
            &mut rustc_hash::FxHashSet::default(),
        ) {
            self.sink
                .push(diagnostics::infer::recursive_type(struct_name, *name_span));
        }
    }

    /// Check if a type transitively contains the target type without passing through Ref.
    /// `target_id` is the qualified name of the type we're checking for recursion.
    /// `current_id` is the qualified name of the type whose fields we're inspecting.
    fn contains_type_without_ref(
        &self,
        store: &Store,
        target_id: &str,
        current_id: &str,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        if !visited.insert(current_id.to_string()) {
            return false; // Already checked this type
        }

        if let Some(fields) = store.fields_of(current_id) {
            for field in fields {
                if self.type_contains_target_without_ref(store, target_id, &field.ty, visited) {
                    return true;
                }
            }
        }

        // Check enum variant payloads.
        // Skip direct self-references (e.g. `Node(Tree, Tree)`) — the emitter wraps
        // those in pointers automatically. Only flag indirect recursion through other
        // types (e.g. `Node(Box<Tree>)` where Box is a value-type struct).
        if let Some(variants) = store.variants_of(current_id) {
            for variant in variants {
                for field in &variant.fields {
                    if let Type::Nominal { id, .. } = field.ty.resolve_in(&self.env)
                        && id == target_id
                    {
                        continue;
                    }
                    if self.type_contains_target_without_ref(store, target_id, &field.ty, visited) {
                        return true;
                    }
                }
            }
        }

        false
    }

    fn type_contains_target_without_ref(
        &self,
        store: &Store,
        target_id: &str,
        ty: &Type,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        match ty {
            Type::Nominal { id, params, .. } => {
                // Ref, Slice, and Map provide heap indirection in Go (pointer,
                // slice header, map pointer) — don't treat as direct containment.
                if matches!(
                    id.as_str(),
                    "Ref" | "prelude.Ref" | "Slice" | "prelude.Slice" | "Map" | "prelude.Map"
                ) {
                    return false;
                }

                if id == target_id {
                    return true;
                }

                for param in params {
                    if self.type_contains_target_without_ref(store, target_id, param, visited) {
                        return true;
                    }
                }

                if (store.fields_of(id).is_some() || store.variants_of(id).is_some())
                    && self.contains_type_without_ref(store, target_id, id, visited)
                {
                    return true;
                }

                false
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|e| self.type_contains_target_without_ref(store, target_id, e, visited)),
            _ => false,
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
        span: &Span,
        doc: &Option<String>,
    ) {
        let qualified_name = self.qualify_name(name);

        if annotation.is_opaque() {
            if self.is_lis(&*store) {
                self.sink
                    .push(diagnostics::infer::opaque_type_outside_typedef(*span));
            }

            let visibility = store
                .get_module(&self.cursor.module_id)
                .expect("current module must exist in store")
                .definitions
                .get(qualified_name.as_str())
                .map(|definition| definition.visibility().clone())
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

            store
                .get_module_mut(&self.cursor.module_id)
                .expect("current module must exist in store")
                .definitions
                .insert(
                    qualified_name,
                    Definition::TypeAlias {
                        visibility,
                        name: name.into(),
                        name_span: *name_span,
                        generics: generics.to_vec(),
                        annotation: annotation.clone(),
                        ty: alias_ty,
                        methods: Default::default(),
                        doc: doc.clone(),
                    },
                );

            return;
        }

        self.scopes.push();

        self.put_in_scope(generics);
        self.validate_generic_bounds(&*store, generics, span);

        let body_ty = self.convert_to_type(&*store, annotation, span);

        if self.is_alias_body_circular(&*store, &body_ty, &qualified_name) {
            self.sink
                .push(diagnostics::infer::circular_type_alias(name, *span));
        }

        let body_ty = if matches!(body_ty, Type::Function { .. }) {
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

        let visibility = store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility().clone())
            .unwrap_or(Visibility::Private);

        if self.is_lis(&*store) && self.type_definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "type alias",
                name,
                *name_span,
            ));
        }

        store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .insert(
                qualified_name,
                Definition::TypeAlias {
                    visibility,
                    name: name.into(),
                    name_span: *name_span,
                    generics: generics.to_vec(),
                    annotation: annotation.clone(),
                    ty: alias_ty,
                    methods: Default::default(),
                    doc: doc.clone(),
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

            if let Some(Definition::TypeAlias { ty, .. }) = store.get_definition(&name) {
                let body = ty.unwrap_forall().clone();
                if Self::type_contains_name(&body, qualified_name) {
                    return true;
                }
                Self::collect_type_refs(&body, &mut to_visit);
            }
        }

        false
    }

    fn type_contains_name(ty: &Type, name: &str) -> bool {
        match ty {
            Type::Nominal {
                id,
                params,
                underlying_ty,
            } => {
                id.as_str() == name
                    || params.iter().any(|p| Self::type_contains_name(p, name))
                    || underlying_ty
                        .as_deref()
                        .is_some_and(|u| Self::type_contains_name(u, name))
            }
            Type::Compound { args, .. } => args.iter().any(|a| Self::type_contains_name(a, name)),
            Type::Tuple(elements) => elements.iter().any(|e| Self::type_contains_name(e, name)),
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| Self::type_contains_name(p, name))
                    || Self::type_contains_name(return_type, name)
            }
            Type::Forall { body, .. } => Self::type_contains_name(body, name),
            _ => false,
        }
    }

    fn collect_type_refs(ty: &Type, refs: &mut Vec<String>) {
        match ty {
            Type::Nominal {
                id,
                params,
                underlying_ty,
            } => {
                refs.push(id.to_string());
                params.iter().for_each(|p| Self::collect_type_refs(p, refs));
                if let Some(u) = underlying_ty {
                    Self::collect_type_refs(u, refs);
                }
            }
            Type::Compound { args, .. } => {
                args.iter().for_each(|a| Self::collect_type_refs(a, refs));
            }
            Type::Tuple(elements) => {
                elements
                    .iter()
                    .for_each(|e| Self::collect_type_refs(e, refs));
            }
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().for_each(|p| Self::collect_type_refs(p, refs));
                Self::collect_type_refs(return_type, refs);
            }
            Type::Forall { body, .. } => Self::collect_type_refs(body, refs),
            _ => {}
        }
    }
}
