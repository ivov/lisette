mod builtins;
mod convert;
mod methods;
mod types;

use rustc_hash::FxHashMap as HashMap;

use deps::TypedefLocator;
use syntax::ast::{
    Annotation, Attribute, AttributeArg, EnumVariant, Expression, FunctionDefinition, Generic,
    Span, StructKind, Visibility as SyntacticVisibility,
};
use syntax::program::{Definition, File, FileImport, Visibility};
use syntax::types::{Symbol, Type};

use super::{FileContextKind, TaskState};
use crate::store::Store;

pub(crate) fn extract_package_directive(source: &str) -> Option<String> {
    for line in source.lines().take(10) {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("// Package:") {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        if !line.starts_with("//") && !line.is_empty() {
            break;
        }
    }
    None
}

pub(super) fn extract_go_name(attributes: &[Attribute]) -> Option<String> {
    attributes
        .iter()
        .filter(|a| a.name == "go")
        .flat_map(|a| a.args.iter())
        .find_map(|arg| {
            if let AttributeArg::String(name) = arg {
                Some(name.clone())
            } else {
                None
            }
        })
}

pub(super) fn extract_attribute_flags(attributes: &[Attribute], name: &str) -> Vec<String> {
    attributes
        .iter()
        .filter(|a| a.name == name)
        .flat_map(|a| {
            a.args.iter().filter_map(|arg| {
                if let AttributeArg::Flag(name) = arg {
                    Some(name.clone())
                } else {
                    None
                }
            })
        })
        .collect()
}

impl TaskState<'_> {
    fn definition_exists(&self, store: &Store, qualified_name: &str) -> bool {
        store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .contains_key(qualified_name)
    }

    fn type_definition_exists(&self, store: &Store, qualified_name: &str) -> bool {
        store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
            .definitions
            .get(qualified_name)
            .is_some_and(|d| {
                matches!(
                    d,
                    Definition::Struct { .. }
                        | Definition::Enum { .. }
                        | Definition::ValueEnum { .. }
                        | Definition::Interface { .. }
                        | Definition::TypeAlias { .. }
                )
            })
    }

    pub fn register_module(&mut self, store: &mut Store, id: &str) {
        let type_name_entries =
            self.with_module_cursor(id, |this| this.collect_module_type_name_entries(store, id));
        self.insert_type_name_entries(store, id, type_name_entries);

        let file_data = self.module_file_data(store, id);

        for (file_id, imports) in &file_data {
            self.register_file_type_definitions(store, id, *file_id, imports);
        }

        for (file_id, imports) in &file_data {
            self.register_file_values(store, id, *file_id, imports);
        }

        let module = store.get_module(id).expect("module must exist");
        let ufcs_entries = crate::call_classification::compute_module_ufcs(module, id);
        self.ufcs_methods.extend(ufcs_entries);
    }

    /// Register a Go module (stdlib or third-party). Unlike regular modules,
    /// Go modules export everything as public and do not put their own module
    /// in scope (no self-references like `MyModule.Type`).
    pub fn parse_and_register_go_module(
        &mut self,
        store: &mut Store,
        module_id: &str,
        source: &str,
        locator: &TypedefLocator,
    ) {
        if store.is_visited(module_id) {
            return;
        }

        store.mark_visited(module_id);
        store.add_module(module_id);

        if let Some(pkg_name) = extract_package_directive(source)
            && module_id.rsplit('/').next() != Some(pkg_name.as_str())
        {
            store
                .go_package_names
                .insert(module_id.to_string(), pkg_name);
        }

        let file_id = store.new_file_id();
        let filename = format!("{}.d.lis", module_id.replace('/', "_"));

        let build_result = syntax::build_ast(source, file_id);
        if build_result.failed() {
            for error in &build_result.errors {
                eprintln!("bindgen: error parsing {}: {:?}", filename, error);
            }
        }

        let file = File {
            id: file_id,
            module_id: module_id.to_string(),
            name: filename,
            source: source.to_string(),
            items: build_result.ast,
        };

        let imports = file.imports();

        for import in &imports {
            if let Some(go_pkg) = import.name.strip_prefix("go:") {
                let import_module_id = format!("go:{}", go_pkg);
                if let deps::TypedefLocatorResult::Found {
                    content: source, ..
                } = locator.find_typedef_content(go_pkg)
                {
                    self.parse_and_register_go_module(store, &import_module_id, &source, locator);
                }
            }
        }

        store.store_file(module_id, file);

        self.with_file_context(
            store,
            module_id,
            file_id,
            &imports,
            FileContextKind::ImportedTypedef,
            |this, store| {
                let items = std::mem::take(
                    &mut store
                        .get_file_mut(file_id)
                        .expect("file must exist after store_file")
                        .items,
                );
                this.register_types_and_values(store, &items, &Visibility::Public);
            },
        );
    }

    fn collect_module_type_name_entries(
        &self,
        store: &Store,
        module_id: &str,
    ) -> Vec<(Symbol, Definition)> {
        let module = store
            .get_module(module_id)
            .expect("module must exist for declaration");
        let mut entries = Vec::new();
        for file in module.files.values() {
            entries.extend(self.collect_type_name_entries(
                &file.items,
                &Visibility::Private,
                false,
            ));
        }
        for file in module.all_typedefs() {
            entries.extend(self.collect_type_name_entries(&file.items, &Visibility::Private, true));
        }
        entries
    }

    fn insert_type_name_entries(
        &mut self,
        store: &mut Store,
        module_id: &str,
        type_name_entries: Vec<(Symbol, Definition)>,
    ) {
        let module = store
            .get_module_mut(module_id)
            .expect("module must exist for declaration");
        for (qualified_name, definition) in type_name_entries {
            module
                .definitions
                .entry(qualified_name)
                .or_insert(definition);
        }
    }

    fn module_file_data(&self, store: &Store, module_id: &str) -> Vec<(u32, Vec<FileImport>)> {
        let module = store
            .get_module(module_id)
            .expect("module must exist for declaration");
        module
            .files
            .iter()
            .chain(module.typedefs.iter())
            .map(|(file_id, f)| (*file_id, f.imports()))
            .collect()
    }

    fn register_file_type_definitions(
        &mut self,
        store: &mut Store,
        module_id: &str,
        file_id: u32,
        imports: &[FileImport],
    ) {
        self.with_file_context(
            store,
            module_id,
            file_id,
            imports,
            FileContextKind::Standard,
            |this, store| {
                let items = std::mem::take(
                    &mut store
                        .get_file_mut(file_id)
                        .expect("file must exist for registration")
                        .items,
                );

                this.register_type_definitions(store, &items);

                store
                    .get_file_mut(file_id)
                    .expect("file must exist after registration")
                    .items = items;
            },
        );
    }

    fn register_file_values(
        &mut self,
        store: &mut Store,
        module_id: &str,
        file_id: u32,
        imports: &[FileImport],
    ) {
        self.with_file_context(
            store,
            module_id,
            file_id,
            imports,
            FileContextKind::Standard,
            |this, store| {
                let items = std::mem::take(
                    &mut store
                        .get_file_mut(file_id)
                        .expect("file must exist for registration")
                        .items,
                );

                this.register_impl_blocks(store, &items);
                this.register_values(store, &items, &Visibility::Private);

                store
                    .get_file_mut(file_id)
                    .expect("file must exist after registration")
                    .items = items;
            },
        );
    }

    pub fn register_types_and_values(
        &mut self,
        store: &mut Store,
        items: &[Expression],
        visibility: &Visibility,
    ) {
        self.register_type_names(store, items, visibility);
        self.register_type_definitions(store, items);
        self.register_impl_blocks(store, items);
        self.register_values(store, items, visibility);
    }

    pub fn register_type_names(
        &mut self,
        store: &mut Store,
        items: &[Expression],
        visibility: &Visibility,
    ) {
        let entries = self.collect_type_name_entries(items, visibility, self.is_d_lis(&*store));
        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");
        for (qualified_name, definition) in entries {
            module
                .definitions
                .entry(qualified_name)
                .or_insert(definition);
        }
    }

    fn collect_type_name_entries(
        &self,
        items: &[Expression],
        visibility: &Visibility,
        is_typedef: bool,
    ) -> Vec<(Symbol, Definition)> {
        let mut entries = Vec::new();

        for item in items {
            let (name, generics, syntactic_visibility) = match item {
                Expression::Enum {
                    name,
                    generics,
                    visibility,
                    ..
                } => (name, generics, *visibility),
                Expression::ValueEnum {
                    name, visibility, ..
                } => (name, &Vec::new() as &Vec<Generic>, *visibility),
                Expression::Struct {
                    name,
                    generics,
                    visibility,
                    ..
                } => (name, generics, *visibility),
                Expression::Interface {
                    name,
                    generics,
                    visibility,
                    ..
                } => (name, generics, *visibility),
                Expression::TypeAlias {
                    name,
                    generics,
                    visibility,
                    ..
                } => (name, generics, *visibility),
                _ => continue,
            };

            let qualified_name = self.qualify_name(name);
            let args: Vec<Type> = generics
                .iter()
                .map(|g| Type::Parameter(g.name.clone()))
                .collect();

            // Canonical form for prelude-registered native types uses the
            // dedicated Simple/Compound variants; everything else remains a
            // nominal Constructor.
            let canonical_ty = if self.cursor.module_id == "prelude" {
                if let Some(simple) = syntax::types::SimpleKind::from_name(name) {
                    debug_assert!(args.is_empty(), "simple kinds have no generics");
                    Type::Simple(simple)
                } else if let Some(compound) = syntax::types::CompoundKind::from_name(name) {
                    Type::Compound {
                        kind: compound,
                        args,
                    }
                } else {
                    Type::Nominal {
                        id: qualified_name.clone(),
                        params: args,
                        underlying_ty: None,
                    }
                }
            } else {
                Type::Nominal {
                    id: qualified_name.clone(),
                    params: args,
                    underlying_ty: None,
                }
            };

            let ty = if generics.is_empty() {
                canonical_ty
            } else {
                Type::Forall {
                    vars: generics.iter().map(|g| g.name.clone()).collect(),
                    body: Box::new(canonical_ty),
                }
            };

            let item_visibility = match visibility {
                Visibility::Local => Visibility::Local,
                _ => {
                    if syntactic_visibility == SyntacticVisibility::Public || is_typedef {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    }
                }
            };

            entries.push((
                qualified_name,
                Definition::Value {
                    visibility: item_visibility,
                    ty,
                    name_span: None,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    doc: None,
                },
            ));
        }

        entries
    }

    pub fn register_type_definitions(&mut self, store: &mut Store, items: &[Expression]) {
        for item in items {
            match item {
                Expression::Enum {
                    name,
                    name_span,
                    generics,
                    variants,
                    span,
                    doc,
                    ..
                } => self.populate_enum(store, name, name_span, generics, variants, span, doc),
                Expression::ValueEnum {
                    name,
                    name_span,
                    underlying_ty,
                    variants,
                    doc,
                    ..
                } => self.populate_value_enum(
                    store,
                    name,
                    name_span,
                    underlying_ty.as_ref(),
                    variants,
                    doc,
                ),
                Expression::Struct {
                    name,
                    name_span,
                    generics,
                    fields,
                    kind,
                    span,
                    doc,
                    ..
                } => {
                    self.populate_struct(store, name, name_span, generics, fields, *kind, span, doc)
                }
                Expression::Interface {
                    name,
                    name_span,
                    generics,
                    parents,
                    method_signatures,
                    span,
                    doc,
                    ..
                } => self.populate_interface(
                    store,
                    name,
                    name_span,
                    generics,
                    parents,
                    method_signatures,
                    span,
                    doc,
                ),
                Expression::TypeAlias {
                    name,
                    name_span,
                    generics,
                    annotation,
                    span,
                    doc,
                    ..
                } => self
                    .populate_type_alias(store, name, name_span, generics, annotation, span, doc),
                _ => (),
            }
        }
    }

    pub fn register_impl_blocks(&mut self, store: &mut Store, items: &[Expression]) {
        for item in items {
            if let Expression::ImplBlock {
                annotation,
                methods,
                generics,
                span,
                ..
            } = item
            {
                self.populate_impl_methods(store, annotation, generics, methods, span);
            }
        }
    }

    fn compute_item_visibility(
        &self,
        store: &Store,
        syntactic: &SyntacticVisibility,
        scope: &Visibility,
    ) -> Visibility {
        match scope {
            Visibility::Local => Visibility::Local,
            _ if *syntactic == SyntacticVisibility::Public || self.is_d_lis(store) => {
                Visibility::Public
            }
            _ => Visibility::Private,
        }
    }

    pub fn register_values(
        &mut self,
        store: &mut Store,
        items: &[Expression],
        visibility: &Visibility,
    ) {
        for item in items {
            match item {
                Expression::Function { .. } => {
                    self.register_function_value(store, item, visibility)
                }
                Expression::Const { .. } => self.register_const_value(store, item, visibility),
                Expression::VariableDeclaration { .. } => {
                    self.register_variable_declaration(store, item, visibility)
                }
                Expression::Struct {
                    kind: StructKind::Tuple,
                    ..
                } => self.register_tuple_struct_constructor(store, item),
                _ => (),
            }
        }
    }

    fn register_function_value(
        &mut self,
        store: &mut Store,
        item: &Expression,
        visibility: &Visibility,
    ) {
        let Expression::Function {
            name,
            name_span,
            attributes,
            generics,
            span,
            body,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        if body.is_noop() && self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::bodyless_function_outside_typedef(*span));
        }

        let fn_sig = item.to_function_signature();
        let qualified_name = self.qualify_name(name);

        self.scopes.push();
        self.put_in_scope(generics);

        let fn_ty = self.extract_function_signature(store, &fn_sig, span);

        self.scopes.pop();

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        if self.is_lis(&*store) && self.definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "function", name, *name_span,
            ));
        }

        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");
        module.definitions.insert(
            qualified_name,
            Definition::Value {
                visibility: item_visibility,
                ty: fn_ty,
                name_span: Some(*name_span),
                allowed_lints: extract_attribute_flags(attributes, "allow"),
                go_hints: extract_attribute_flags(attributes, "go"),
                go_name: extract_go_name(attributes),
                doc: doc.clone(),
            },
        );
    }

    fn register_const_value(
        &mut self,
        store: &mut Store,
        item: &Expression,
        visibility: &Visibility,
    ) {
        let Expression::Const {
            identifier,
            identifier_span,
            annotation: maybe_annotation,
            expression,
            span,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        let has_value = !expression.is_noop();

        if !has_value && self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::valueless_const_outside_typedef(*span));
        }

        if !has_value && maybe_annotation.is_none() && self.is_d_lis(&*store) {
            self.sink
                .push(diagnostics::infer::valueless_const_missing_annotation(
                    *span,
                ));
        }

        let qualified_name = self.qualify_name(identifier);

        let before = self.sink.len();
        let const_ty = if let Some(annotation) = maybe_annotation {
            self.convert_to_type(store, annotation, span)
        } else {
            self.type_from_literal_expression(expression)
                .unwrap_or_else(|| self.new_type_var())
        };
        self.sink.truncate(before);

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        if self.is_lis(&*store) && self.definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "constant",
                identifier,
                *identifier_span,
            ));
        }

        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");
        module.const_names.insert(qualified_name.clone());
        module.definitions.insert(
            qualified_name,
            Definition::Value {
                visibility: item_visibility,
                ty: const_ty,
                name_span: Some(*identifier_span),
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                doc: doc.clone(),
            },
        );
    }

    fn register_variable_declaration(
        &mut self,
        store: &mut Store,
        item: &Expression,
        visibility: &Visibility,
    ) {
        let Expression::VariableDeclaration {
            name,
            name_span,
            annotation,
            span,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        if self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::variable_declaration_outside_typedef(
                    *span,
                ));
        }

        let qualified_name = self.qualify_name(name);
        let var_ty = self.convert_to_type(&*store, annotation, span);

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");
        module.definitions.insert(
            qualified_name,
            Definition::Value {
                visibility: item_visibility,
                ty: var_ty,
                name_span: Some(*name_span),
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                doc: doc.clone(),
            },
        );
    }

    fn register_tuple_struct_constructor(&mut self, store: &mut Store, item: &Expression) {
        let Expression::Struct {
            name,
            generics,
            fields,
            kind: StructKind::Tuple,
            span,
            ..
        } = item
        else {
            return;
        };

        let qualified_name = self.qualify_name(name);
        let struct_ty = store
            .get_type(&qualified_name)
            .expect("struct type scheme must exist")
            .clone();

        self.scopes.push();
        self.put_in_scope(generics);

        let field_types: Vec<Type> = fields
            .iter()
            .map(|f| self.convert_to_type(&*store, &f.annotation, span))
            .collect();

        self.scopes.pop();

        let constructor_ty =
            tuple_struct_constructor_type_from_fields(&field_types, &struct_ty, generics);

        let scope = self.scopes.current_mut();
        scope
            .values
            .insert(qualified_name.to_string(), constructor_ty.clone());
        scope
            .values
            .insert(name.to_string(), constructor_ty.clone());

        let module = store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store");
        if let Some(Definition::Struct { constructor, .. }) =
            module.definitions.get_mut(qualified_name.as_str())
        {
            *constructor = Some(constructor_ty);
        }
    }

    pub(crate) fn extract_function_signature(
        &mut self,
        store: &Store,
        function: &FunctionDefinition,
        span: &Span,
    ) -> Type {
        let generics = &function.generics;

        self.scopes.push();
        self.put_in_scope(generics);

        let mut bounds = vec![];

        for g in generics {
            let qualified_name = self.qualify_name(&g.name);

            for b in &g.bounds {
                let bound_ty = self.register_bound_annotation(store, b, span);

                self.scopes
                    .current_mut()
                    .trait_bounds
                    .get_or_insert_with(HashMap::default)
                    .entry(qualified_name.clone())
                    .or_default()
                    .push(bound_ty.clone());

                bounds.push(syntax::types::Bound {
                    param_name: g.name.clone(),
                    generic: Type::Parameter(g.name.clone()),
                    ty: bound_ty,
                });
            }
        }

        let before = self.sink.len();

        let param_types: Vec<Type> = function
            .params
            .iter()
            .map(|binding| {
                binding
                    .annotation
                    .as_ref()
                    .map(|a| self.convert_to_type(store, a, span))
                    .unwrap_or_else(|| self.new_type_var())
            })
            .collect();

        let return_ty = match &function.annotation {
            Annotation::Unknown => self.type_unit(),
            _ => self.convert_to_type(store, &function.annotation, span),
        };

        self.sink.truncate(before);

        self.scopes.pop();

        let param_mutability: Vec<bool> = function.params.iter().map(|b| b.mutable).collect();

        let base_fn_ty = Type::Function {
            params: param_types,
            param_mutability,
            bounds,
            return_type: return_ty.into(),
        };

        if generics.is_empty() {
            base_fn_ty
        } else {
            Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(base_fn_ty),
            }
        }
    }
}

pub(super) fn enum_variant_constructor_type(
    enum_variant: &EnumVariant,
    enum_ty: &Type,
    generics: &[Generic],
) -> Type {
    if enum_variant.fields.is_empty() {
        return enum_ty.clone();
    }

    let return_type = match enum_ty {
        Type::Forall { body, .. } => body.as_ref().clone(),
        _ => enum_ty.clone(),
    };

    let fn_ty = Type::Function {
        param_mutability: vec![false; enum_variant.fields.len()],
        params: enum_variant.fields.iter().map(|f| f.ty.clone()).collect(),
        bounds: Default::default(),
        return_type: return_type.into(),
    };

    if generics.is_empty() {
        fn_ty
    } else {
        Type::Forall {
            vars: generics.iter().map(|g| g.name.clone()).collect(),
            body: Box::new(fn_ty),
        }
    }
}

fn tuple_struct_constructor_type_from_fields(
    field_types: &[Type],
    struct_ty: &Type,
    generics: &[Generic],
) -> Type {
    let return_type = match struct_ty {
        Type::Forall { body, .. } => body.as_ref().clone(),
        _ => struct_ty.clone(),
    };

    let fn_ty = Type::Function {
        param_mutability: vec![false; field_types.len()],
        params: field_types.to_vec(),
        bounds: Default::default(),
        return_type: return_type.into(),
    };

    if generics.is_empty() {
        fn_ty
    } else {
        Type::Forall {
            vars: generics.iter().map(|g| g.name.clone()).collect(),
            body: Box::new(fn_ty),
        }
    }
}

pub(super) fn wrap_with_impl_generics(
    fn_ty: &Type,
    generics: &[Generic],
    impl_bounds: &[syntax::types::Bound],
) -> Type {
    if generics.is_empty() {
        return fn_ty.clone();
    }

    let impl_vars: Vec<syntax::EcoString> = generics.iter().map(|g| g.name.clone()).collect();

    let add_impl_bounds = |existing_bounds: &[syntax::types::Bound]| -> Vec<syntax::types::Bound> {
        impl_bounds
            .iter()
            .cloned()
            .chain(existing_bounds.iter().cloned())
            .collect()
    };

    match fn_ty {
        Type::Forall { vars, body } => {
            let new_body = match body.as_ref() {
                Type::Function {
                    params,
                    param_mutability,
                    bounds,
                    return_type,
                } => Type::Function {
                    params: params.clone(),
                    param_mutability: param_mutability.clone(),
                    bounds: add_impl_bounds(bounds),
                    return_type: return_type.clone(),
                },
                _ => *body.clone(),
            };
            Type::Forall {
                vars: impl_vars.into_iter().chain(vars.clone()).collect(),
                body: Box::new(new_body),
            }
        }
        Type::Function {
            params,
            param_mutability,
            bounds,
            return_type,
        } => Type::Forall {
            vars: impl_vars,
            body: Box::new(Type::Function {
                params: params.clone(),
                param_mutability: param_mutability.clone(),
                bounds: add_impl_bounds(bounds),
                return_type: return_type.clone(),
            }),
        },
        _ => Type::Forall {
            vars: impl_vars,
            body: Box::new(fn_ty.clone()),
        },
    }
}

fn type_contains_constructor(target_id: &str, ty: &Type) -> bool {
    walk_type(ty, &|id, _| id == target_id)
}

/// Check if a type contains a recursive generic instantiation.
/// E.g., a method on `Box<T>` returning `Box<Box<T>>` creates a Go instantiation cycle.
/// Returns true if `ty` contains `target_id` nested within itself (e.g. `Box<Box<T>>`).
pub(super) fn has_recursive_instantiation(target_id: &str, ty: &Type) -> bool {
    walk_type(ty, &|id, params| {
        id == target_id
            && params
                .iter()
                .any(|p| type_contains_constructor(target_id, p))
    })
}

fn walk_type(ty: &Type, predicate: &dyn Fn(&str, &[Type]) -> bool) -> bool {
    match ty {
        Type::Nominal { id, params, .. } => {
            predicate(id, params) || params.iter().any(|p| walk_type(p, predicate))
        }
        Type::Function {
            params,
            return_type,
            ..
        } => params.iter().any(|p| walk_type(p, predicate)) || walk_type(return_type, predicate),
        Type::Tuple(elems) => elems.iter().any(|e| walk_type(e, predicate)),
        Type::Forall { body, .. } => walk_type(body, predicate),
        _ => false,
    }
}
