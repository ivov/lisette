mod builtins;
mod convert;
pub(crate) mod derived_attributes;
mod display;
mod equality;
mod generic_bounds;
mod impl_bounds;
mod iterate;
mod methods;
pub(crate) mod test_functions;
mod types;

use std::path::PathBuf;

use deps::TypedefLocator;

use crate::diagnostics::{GoImportSite, emit_for_locator_result};
use syntax::ast::{
    Annotation, Attribute, AttributeArg, Binding, EnumVariant, Expression, Generic, Span,
    StructFields, VariantFields, Visibility as SyntacticVisibility,
};
use syntax::attributes::struct_attribute_forces_field_export;
use syntax::program::{
    AliasKind, Attributes, Definition, DefinitionBody, File, FileImport, TypeAttribute, Visibility,
};
use syntax::types::{Bound, FunctionParameter, Symbol, Type};

use super::{FileContext, TaskState, resolved_generic_bounds};
use crate::store::Store;

struct RegistrationFile {
    id: u32,
    imports: Vec<FileImport>,
    items: Vec<Expression>,
}

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

fn extract_go_name(attributes: &[Attribute]) -> Option<String> {
    attributes
        .iter()
        .filter(|a| a.name == "go")
        .filter(|a| {
            !a.args
                .iter()
                .any(|arg| matches!(arg, AttributeArg::Flag(_)))
        })
        .find_map(|a| {
            a.args.iter().find_map(|arg| match arg {
                AttributeArg::String(name) => Some(name.clone()),
                _ => None,
            })
        })
}

/// The recipe string from `#[go(collapsed_type_params, "...")]`. This is
/// Go's full type-param in declaration order, each entry as a Lisette type.
fn extract_go_type_param_recipe(attributes: &[Attribute]) -> Option<String> {
    attributes
        .iter()
        .filter(|a| a.name == "go")
        .filter(|a| {
            a.args
                .iter()
                .any(|arg| matches!(arg, AttributeArg::Flag(f) if f == "collapsed_type_params"))
        })
        .find_map(|a| {
            a.args.iter().find_map(|arg| match arg {
                AttributeArg::String(recipe) => Some(recipe.clone()),
                _ => None,
            })
        })
}

fn has_display_attribute(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "display")
}

fn has_closed_domain_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "closed_domain")
}

fn has_anon_struct_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "anon_struct")
}

fn has_hidden_embed_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "hidden_embed")
}

fn has_unexported_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "unexported")
}

fn has_serialization_attribute(attributes: &[Attribute]) -> bool {
    attributes.iter().any(struct_attribute_forces_field_export)
}

fn collect_enum_attributes(attributes: &[Attribute]) -> Attributes {
    let mut map = Attributes::default();
    if has_display_attribute(attributes) {
        map.insert(TypeAttribute::Display);
    }
    map
}

fn collect_struct_attributes(attributes: &[Attribute]) -> Attributes {
    let mut map = Attributes::default();
    if has_display_attribute(attributes) {
        map.insert(TypeAttribute::Display);
    }
    if has_closed_domain_attribute(attributes) {
        map.insert(TypeAttribute::ClosedDomain);
    }
    if has_anon_struct_attribute(attributes) {
        map.insert(TypeAttribute::AnonStruct);
    }
    if has_hidden_embed_attribute(attributes) {
        map.insert(TypeAttribute::HiddenEmbed);
    }
    if has_serialization_attribute(attributes) {
        map.insert(TypeAttribute::Serialized);
    }
    map
}

fn canonical_const_literal(expression: &Expression) -> Option<syntax::ast::Literal> {
    use syntax::ast::{Literal, UnaryOperator};
    match expression.unwrap_parens() {
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { value, .. } => Some(Literal::Integer {
                value: *value,
                text: None,
            }),
            Literal::Float { value, .. } => Some(Literal::Float {
                value: *value,
                text: None,
            }),
            Literal::Boolean(b) => Some(Literal::Boolean(*b)),
            Literal::String { value, .. } => Some(Literal::String {
                value: value.clone(),
                raw: false,
            }),
            Literal::Char(c) => Some(Literal::Char(c.clone())),
            _ => None,
        },
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => match canonical_const_literal(expression)? {
            Literal::Integer { value, .. } => Some(Literal::Integer {
                value: value.wrapping_neg(),
                text: None,
            }),
            Literal::Float { value, .. } => Some(Literal::Float {
                value: -value,
                text: None,
            }),
            _ => None,
        },
        _ => None,
    }
}

const KNOWN_GO_HINTS: &[&str] = &[
    "anon_struct",
    "bit_flag_set",
    "closed_domain",
    "collapsed_type_params",
    "comma_ok",
    "hidden_embed",
    "sentinel_minus_one",
    "unexported",
];

fn check_go_hints(attributes: &[Attribute], sink: &diagnostics::LocalSink) {
    for attribute in attributes.iter().filter(|a| a.name == "go") {
        for arg in &attribute.args {
            if let AttributeArg::Flag(flag) = arg
                && !KNOWN_GO_HINTS.contains(&flag.as_str())
            {
                sink.push(diagnostics::attribute::unknown_go_hint(
                    &attribute.span,
                    flag,
                ));
            }
        }
    }
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

fn extract_attribute_string(attributes: &[Attribute], name: &str) -> Option<String> {
    attributes.iter().filter(|a| a.name == name).find_map(|a| {
        a.args.iter().find_map(|arg| match arg {
            AttributeArg::String(s) => Some(s.clone()),
            _ => None,
        })
    })
}

fn seal_method_key(
    is_d_lis: bool,
    attributes: &[Attribute],
    module_id: &str,
    name: &str,
) -> ecow::EcoString {
    let id = if is_d_lis {
        extract_attribute_string(attributes, "go").unwrap_or_else(|| format!("{module_id}.{name}"))
    } else {
        format!("{module_id}.{name}")
    };
    crate::checker::sealing::unexported_key(&id)
}

impl TaskState {
    fn definition_exists(&self, store: &Store, qualified_name: &str) -> bool {
        self.current_module(store)
            .definitions
            .contains_key(qualified_name)
    }

    fn type_definition_exists(&self, store: &Store, qualified_name: &str) -> bool {
        self.current_module(store)
            .definitions
            .get(qualified_name)
            .is_some_and(|d| {
                matches!(
                    d.body,
                    DefinitionBody::Struct { .. }
                        | DefinitionBody::Enum { .. }
                        | DefinitionBody::Interface { .. }
                        | DefinitionBody::TypeAlias { .. }
                )
            })
    }

    pub fn register_module(&mut self, store: &mut Store, id: &str) {
        self.predeclare_module_types(store, id);
        self.register_predeclared_module(store, id);
    }

    pub(crate) fn register_predeclared_module(&mut self, store: &mut Store, id: &str) {
        let mut files = {
            let module = store
                .get_module_mut(id)
                .expect("module must exist for registration");
            module
                .files
                .values_mut()
                .map(|file| RegistrationFile {
                    id: file.id,
                    imports: file.imports(),
                    items: std::mem::take(&mut file.items),
                })
                .collect::<Vec<_>>()
        };

        for file in &files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    module_id: id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| this.register_type_aliases(store, &file.items),
            );
        }

        for file in &files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    module_id: id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| this.register_type_bodies(store, &file.items),
            );
        }

        for file in &files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    module_id: id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| {
                    this.check_type_generic_bounds(store, &file.items);
                    this.register_impl_blocks(store, &file.items);
                    this.register_values(store, &file.items, &Visibility::Private);
                },
            );
        }

        for file in &mut files {
            store
                .get_file_mut(file.id)
                .expect("registered file must remain in the store")
                .items = std::mem::take(&mut file.items);
        }

        self.register_module_derived_attributes(store, id);
        self.validate_module_embeds(store, id);
        self.check_module_recursive_types(store, id);

        self.register_module_tests(store, id);
        self.populate_module_generic_bounds(store, id);
    }

    pub(crate) fn predeclare_module_types(&mut self, store: &mut Store, id: &str) {
        let type_name_entries =
            self.with_module_cursor(id, |this| this.collect_module_type_name_entries(store, id));
        self.insert_type_name_entries(store, id, type_name_entries);
    }

    fn populate_module_generic_bounds(&self, store: &mut Store, module_id: &str) {
        let Some(module) = store.get_module_mut(module_id) else {
            return;
        };
        for file in module.files.values_mut() {
            for item in &mut file.items {
                populate_expression_generic_bounds(item, &self.facts.bound_types);
            }
        }
    }

    /// Resolve each item's generic bounds from the per-module pass results.
    /// Test harnesses that emit a typed AST directly bypass that pass.
    pub fn populate_item_generic_bounds(&self, items: &mut [Expression]) {
        for item in items {
            populate_expression_generic_bounds(item, &self.facts.bound_types);
        }
    }

    /// Register a Go module (stdlib or third-party). Unlike regular modules,
    /// Go modules export everything as public and do not put their own module
    /// in scope (no self-references like `MyModule.Type`). `cache_path` is the
    /// on-disk typedef location, or `None` for embedded stdlib typedefs.
    pub fn parse_and_register_go_module(
        &mut self,
        store: &mut Store,
        module_id: &str,
        source: &str,
        cache_path: Option<PathBuf>,
        locator: &TypedefLocator,
    ) {
        if store.has(module_id) {
            return;
        }

        store.add_module(module_id);

        if let Some(pkg_name) = extract_package_directive(source) {
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
            name: filename.clone(),
            display_path: filename,
            source_path: cache_path,
            source: source.to_string(),
            items: build_result.ast,
            file_comment: build_result.file_comment,
        };

        let imports = file.imports();

        let replace_importer = module_id.strip_prefix("go:").filter(|pkg| {
            matches!(
                locator.validate_declaration(pkg),
                deps::DeclarationStatus::DeclaredReplacement { .. }
            )
        });

        for import in &imports {
            if let Some(go_pkg) = import.name.strip_prefix("go:") {
                if matches!(import.alias, Some(syntax::ast::ImportAlias::Blank(_))) {
                    continue;
                }

                let import_module_id = format!("go:{}", go_pkg);

                if store.has(&import_module_id) {
                    continue;
                }

                match locator.find_typedef_content(go_pkg) {
                    deps::TypedefLocatorResult::Found { content, origin } => {
                        self.parse_and_register_go_module(
                            store,
                            &import_module_id,
                            content.as_ref(),
                            origin.into_cache_path(),
                            locator,
                        );
                    }
                    other => {
                        emit_for_locator_result(
                            &other,
                            &GoImportSite {
                                import_name: &import.name,
                                go_pkg,
                                name_span: Some(import.name_span),
                                target: locator.target(),
                                standalone_mode: false,
                                replace_importer,
                            },
                            &self.sink,
                        );
                    }
                }
            }
        }

        store.store_file(file);

        self.with_file_context_mut(
            store,
            FileContext::ImportedTypedef {
                module_id,
                file_id,
                imports: &imports,
            },
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
        for file in module.source_files() {
            entries.extend(self.collect_type_name_entries(
                &file.items,
                &Visibility::Private,
                false,
            ));
        }
        for file in module.typedef_files() {
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

    pub fn register_types_and_values(
        &mut self,
        store: &mut Store,
        items: &[Expression],
        visibility: &Visibility,
    ) {
        self.register_type_names(store, items, visibility);
        self.register_type_definitions(store, items);
        self.check_type_generic_bounds(store, items);
        self.register_impl_blocks(store, items);
        self.register_values(store, items, visibility);
        self.register_item_derived_attributes(store, items);
        let module_id = self.cursor.module_id.clone();
        self.validate_module_embeds(store, &module_id);
        self.check_module_recursive_types(store, &module_id);
    }

    pub(crate) fn register_type_names(
        &mut self,
        store: &mut Store,
        items: &[Expression],
        visibility: &Visibility,
    ) {
        let entries = self.collect_type_name_entries(items, visibility, self.is_d_lis(&*store));
        let module = self.current_module_mut(store);
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
            let (name, generics, syntactic_visibility, attributes): (_, _, _, &[Attribute]) =
                match item {
                    Expression::Enum {
                        name,
                        generics,
                        visibility,
                        attributes,
                        ..
                    } => (name, generics, *visibility, attributes),
                    Expression::Struct {
                        name,
                        generics,
                        visibility,
                        attributes,
                        ..
                    } => (name, generics, *visibility, attributes),
                    Expression::Interface {
                        name,
                        generics,
                        visibility,
                        ..
                    } => (name, generics, *visibility, &[]),
                    Expression::TypeAlias {
                        name,
                        generics,
                        visibility,
                        attributes,
                        ..
                    } => (name, generics, *visibility, attributes),
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
                    }
                }
            } else {
                Type::Nominal {
                    id: qualified_name.clone(),
                    params: args,
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
                // A Go unexported type stays module-private even in a typedef,
                // mirroring Go's lexical export rule.
                _ if has_unexported_attribute(attributes) => Visibility::Private,
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
                Definition {
                    visibility: item_visibility,
                    ty,
                    name_span: None,
                    doc: None,
                    body: DefinitionBody::Value {
                        kind: syntax::program::ValueKind::Runtime,
                        allowed_lints: vec![],
                        go_hints: vec![],
                        go_name: None,
                        go_type_param_recipe: None,
                    },
                },
            ));
        }

        entries
    }

    fn check_go_hints_in_items(&self, items: &[Expression]) {
        for item in items {
            match item {
                Expression::Enum { attributes, .. }
                | Expression::Struct { attributes, .. }
                | Expression::TypeAlias { attributes, .. }
                | Expression::Function { attributes, .. } => {
                    check_go_hints(attributes, &self.sink);
                }
                Expression::ImplBlock {
                    methods: functions, ..
                }
                | Expression::Interface {
                    method_signatures: functions,
                    ..
                } => {
                    for function in functions {
                        if let Expression::Function { attributes, .. } = function {
                            check_go_hints(attributes, &self.sink);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn register_type_definitions(&mut self, store: &mut Store, items: &[Expression]) {
        self.register_type_aliases(store, items);
        self.register_type_bodies(store, items);
    }

    fn register_type_aliases(&mut self, store: &mut Store, items: &[Expression]) {
        for item in items {
            if matches!(item, Expression::TypeAlias { .. }) {
                self.populate_type_alias(store, item);
            }
        }
    }

    fn register_type_bodies(&mut self, store: &mut Store, items: &[Expression]) {
        self.check_go_hints_in_items(items);
        for item in items {
            match item {
                Expression::Enum { .. } => self.populate_enum(store, item),
                Expression::Struct { .. } => self.populate_struct(store, item),
                Expression::Interface { .. } => self.populate_interface(store, item),
                _ => (),
            }
        }
    }

    fn check_type_generic_bounds(&mut self, store: &Store, items: &[Expression]) {
        for item in items {
            let (name, span) = match item {
                Expression::Enum { name, span, .. }
                | Expression::Struct { name, span, .. }
                | Expression::Interface { name, span, .. }
                | Expression::TypeAlias { name, span, .. } => (name, *span),
                _ => continue,
            };
            let Some(definition) = store.get_definition(&self.qualify_name(name)) else {
                continue;
            };
            let generics = definition
                .body
                .generics()
                .map(<[Generic]>::to_vec)
                .unwrap_or_default();
            let value_types = declaration_value_position_types(definition);

            self.with_scope(|this| {
                this.put_in_scope(&generics);
                this.record_resolved_generic_bounds(&generics);
                this.check_transitive_generic_bounds(store, &generics, span);
                this.check_value_position_bounds(store, &generics, &value_types);
            });
        }
    }

    pub(crate) fn register_impl_blocks(&mut self, store: &mut Store, items: &[Expression]) {
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

    pub(crate) fn register_values(
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
                    fields: StructFields::Tuple(_),
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
            params,
            return_annotation,
            span,
            body,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        if body.definition().is_none() && self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::bodyless_function_outside_typedef(*span));
        }

        let qualified_name = self.qualify_name(name);

        let fn_ty = self.with_scope(|this| {
            this.put_in_scope(generics);

            let test_params;
            let params: &[Binding] = if syntax::attributes::has_test_attribute(attributes) {
                test_params = test_functions::normalize_test_params(params.clone(), true);
                &test_params
            } else {
                params
            };

            let fn_ty =
                this.extract_signature_parts(store, generics, params, return_annotation, span);

            let (signature_pairs, signature_bounds) =
                function_signature_pairs(&fn_ty, params, *span);
            for bound in &signature_bounds {
                this.record_generic_bound(&bound.param_name, bound.ty.clone());
            }
            this.check_value_position_bounds(store, &[], &signature_pairs);
            fn_ty
        });

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        if self.is_lis(&*store) && self.definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "function", name, *name_span,
            ));
        }

        let module = self.current_module_mut(store);
        module.definitions.insert(
            qualified_name,
            Definition {
                visibility: item_visibility,
                ty: fn_ty,
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: extract_attribute_flags(attributes, "allow"),
                    go_hints: extract_attribute_flags(attributes, "go"),
                    go_name: extract_go_name(attributes),
                    go_type_param_recipe: extract_go_type_param_recipe(attributes),
                },
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

        let has_value = expression.value().is_some();

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

        let const_ty = self.without_diagnostics(|this| {
            if let Some(annotation) = maybe_annotation {
                this.convert_to_type(store, annotation, span)
            } else {
                expression
                    .value()
                    .and_then(|value| this.type_from_literal_expression(value))
                    .unwrap_or_else(|| this.new_type_var())
            }
        });

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        if self.is_lis(&*store) && self.definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "constant",
                identifier,
                *identifier_span,
            ));
        }

        let kind = match expression.value().and_then(canonical_const_literal) {
            Some(value) => syntax::program::ValueKind::Constant(value),
            None => syntax::program::ValueKind::ConstantDeclaration,
        };

        self.current_module_mut(store).definitions.insert(
            qualified_name,
            Definition {
                visibility: item_visibility,
                ty: const_ty,
                name_span: Some(*identifier_span),
                doc: doc.clone(),
                body: DefinitionBody::Value {
                    kind,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
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

        let module = self.current_module_mut(store);
        module.definitions.insert(
            qualified_name,
            Definition {
                visibility: item_visibility,
                ty: var_ty,
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
            },
        );
    }

    fn register_tuple_struct_constructor(&mut self, store: &mut Store, item: &Expression) {
        let Expression::Struct {
            name,
            fields: StructFields::Tuple(_),
            ..
        } = item
        else {
            return;
        };

        let qualified_name = self.qualify_name(name);
        let constructor_ty = store
            .get_definition(&qualified_name)
            .and_then(Definition::constructor_type)
            .expect("tuple struct definition must have a constructor type");

        let scope = self.scopes.current_mut();
        scope.insert_value(qualified_name.to_string(), constructor_ty.clone());
        scope.insert_value(name.to_string(), constructor_ty.clone());
    }

    pub(crate) fn extract_signature_parts(
        &mut self,
        store: &Store,
        generics: &[Generic],
        params: &[Binding],
        return_annotation: &Annotation,
        span: &Span,
    ) -> Type {
        let (generics, bounds, param_types, return_ty) = self.with_scope(|this| {
            this.put_in_scope(generics);
            let generics = this.resolve_generic_bounds(store, generics, span);
            this.check_transitive_generic_bounds(store, &generics, *span);
            let bounds = resolved_generic_bounds(&generics);

            let (param_types, return_ty) = this.without_diagnostics(|this| {
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|binding| match &binding.annotation {
                        Some(a) => this.convert_variadic_to_type(store, a, span),
                        None if !binding.ty.is_uninferred() => binding.ty.clone(),
                        None => this.new_type_var(),
                    })
                    .collect();

                let return_ty = match return_annotation {
                    Annotation::Unknown => this.type_unit(),
                    _ => this.convert_to_type(store, return_annotation, span),
                };
                (param_types, return_ty)
            });
            (generics, bounds, param_types, return_ty)
        });

        let function_params = param_types
            .into_iter()
            .zip(params)
            .map(|(ty, binding)| {
                FunctionParameter::named(ty, binding.pattern.get_identifier(), binding.is_mutable())
            })
            .collect();

        let base_fn_ty = Type::function(function_params, bounds, return_ty.into());

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

fn declaration_value_position_types(definition: &Definition) -> Vec<(Type, Span)> {
    match &definition.body {
        DefinitionBody::Struct { fields, .. } => fields
            .iter()
            .map(|field| (field.ty.clone(), field.annotation.get_span()))
            .collect(),
        DefinitionBody::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| variant_field_types(&variant.fields))
            .collect(),
        DefinitionBody::TypeAlias { alias, .. } => alias_body_types(alias),
        _ => Vec::new(),
    }
}

fn function_signature_pairs(
    fn_ty: &Type,
    params: &[Binding],
    fallback: Span,
) -> (Vec<(Type, Span)>, Vec<Bound>) {
    let Type::Function(function) = fn_ty.unwrap_forall() else {
        return (Vec::new(), Vec::new());
    };
    let pairs: Vec<(Type, Span)> = function
        .params
        .iter()
        .enumerate()
        .map(|(index, param_ty)| {
            let span = params
                .get(index)
                .and_then(|binding| binding.annotation.as_ref())
                .map_or(fallback, Annotation::get_span);
            (param_ty.ty.clone(), span)
        })
        .collect();
    (pairs, function.bounds.clone())
}

fn variant_field_types(fields: &VariantFields) -> Vec<(Type, Span)> {
    match fields {
        VariantFields::Unit => Vec::new(),
        VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields
            .iter()
            .map(|field| (field.ty.clone(), field.annotation.get_span()))
            .collect(),
    }
}

fn alias_body_types(alias: &AliasKind) -> Vec<(Type, Span)> {
    match alias {
        AliasKind::Opaque(_) => Vec::new(),
        AliasKind::Transparent { annotation, target } => {
            vec![(target.clone(), annotation.get_span())]
        }
    }
}

fn populate_expression_generic_bounds(
    expression: &mut Expression,
    bound_types: &rustc_hash::FxHashMap<Span, Type>,
) {
    match expression {
        Expression::Function { generics, .. }
        | Expression::Struct { generics, .. }
        | Expression::Enum { generics, .. }
        | Expression::TypeAlias { generics, .. } => populate_generic_bounds(generics, bound_types),
        Expression::ImplBlock {
            generics, methods, ..
        } => {
            populate_generic_bounds(generics, bound_types);
            for method in methods {
                populate_expression_generic_bounds(method, bound_types);
            }
        }
        Expression::Interface {
            generics,
            method_signatures,
            ..
        } => {
            populate_generic_bounds(generics, bound_types);
            for method in method_signatures {
                populate_expression_generic_bounds(method, bound_types);
            }
        }
        _ => {}
    }
}

fn populate_generic_bounds(
    generics: &mut [Generic],
    bound_types: &rustc_hash::FxHashMap<Span, Type>,
) {
    for generic in generics {
        generic.resolve_bounds_with(|bound| {
            bound_types
                .get(&bound.get_span())
                .cloned()
                .unwrap_or(Type::Error)
        });
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

    let fn_ty = Type::function(
        enum_variant
            .fields
            .iter()
            .map(|field| FunctionParameter::new(field.ty.clone(), false))
            .collect(),
        Default::default(),
        return_type.into(),
    );

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
                Type::Function(f) => f.rebuild(
                    f.params.clone(),
                    add_impl_bounds(&f.bounds),
                    f.return_type.clone(),
                ),
                _ => *body.clone(),
            };
            Type::Forall {
                vars: impl_vars.into_iter().chain(vars.clone()).collect(),
                body: Box::new(new_body),
            }
        }
        Type::Function(f) => Type::Forall {
            vars: impl_vars,
            body: Box::new(f.rebuild(
                f.params.clone(),
                add_impl_bounds(&f.bounds),
                f.return_type.clone(),
            )),
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
    if let Type::Nominal { id, params, .. } = ty
        && predicate(id, params)
    {
        return true;
    }
    ty.children().iter().any(|c| walk_type(c, predicate))
}
