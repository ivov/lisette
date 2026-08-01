use super::*;

impl TaskState {
    pub(super) fn collect_module_type_name_entries(
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

    pub(super) fn insert_type_name_entries(
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

    pub(super) fn register_type_aliases(&mut self, store: &mut Store, items: &[Expression]) {
        for item in items {
            if matches!(item, Expression::TypeAlias { .. }) {
                self.populate_type_alias(store, item);
            }
        }
    }

    pub(super) fn register_type_bodies(&mut self, store: &mut Store, items: &[Expression]) {
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

    pub(super) fn check_type_generic_bounds(&mut self, store: &Store, items: &[Expression]) {
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
}
