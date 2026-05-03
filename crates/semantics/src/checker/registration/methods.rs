use ecow::EcoString;
use syntax::ast::{
    Annotation, Expression, Generic, ParentInterface, Pattern, Span,
    Visibility as SyntacticVisibility,
};
use syntax::program::{Definition, DefinitionBody, Interface, Visibility};
use syntax::types::{Symbol, Type, unqualified_name};

use super::{extract_attribute_flags, has_recursive_instantiation, wrap_with_impl_generics};
use crate::checker::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    /// Register an instance method on the receiver type's definition.
    /// Returns `false` if the receiver was not found or is a ValueEnum (caller should skip).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_register_instance_method(
        &mut self,
        store: &mut Store,
        module_id: &str,
        receiver_qualified_name: &str,
        type_name: &str,
        fn_name: &str,
        fn_name_span: Span,
        method_ty: &Type,
    ) -> bool {
        let module = store
            .get_module_mut(module_id)
            .expect("current module must exist in store");

        let Some(definition) = module.definitions.get_mut(receiver_qualified_name) else {
            // Receiver type not found in current module (e.g. resolved
            // to a same-named type in another module). Skip registering
            // the method to avoid false duplicate errors.
            return false;
        };

        if let DefinitionBody::Struct { fields, .. } = &definition.body
            && fields.iter().any(|f| f.name == fn_name)
        {
            self.sink.push(diagnostics::infer::method_shadows_field(
                type_name,
                fn_name,
                fn_name_span,
            ));
        }

        if let DefinitionBody::Enum { variants, .. } = &definition.body {
            for variant in variants {
                if variant.fields.is_struct() && variant.fields.iter().any(|f| f.name == fn_name) {
                    self.sink.push(diagnostics::infer::method_shadows_field(
                        type_name,
                        fn_name,
                        fn_name_span,
                    ));
                    break;
                }
            }
        }

        let is_value_enum = matches!(definition.body, DefinitionBody::ValueEnum { .. });

        if let Some(methods) = definition.methods_mut() {
            methods.insert(fn_name.into(), method_ty.clone());
        }

        !is_value_enum
    }

    #[allow(clippy::too_many_arguments)]
    fn check_duplicate_method(
        &self,
        store: &Store,
        module_id: &str,
        receiver_qualified_name: &str,
        type_name: &str,
        fn_name: &str,
        fn_name_span: Span,
        impl_generics_empty: bool,
    ) {
        let module_qualified_name = Symbol::from_parts(module_id, type_name).with_segment(fn_name);

        let module = store
            .get_module(module_id)
            .expect("current module must exist in store");

        if !module
            .definitions
            .contains_key(module_qualified_name.as_str())
        {
            return;
        }

        let is_cross_specialization = impl_generics_empty
            && matches!(
                module.definitions.get(receiver_qualified_name).map(|d| &d.body),
                Some(DefinitionBody::Struct { generics: struct_generics, .. })
                    if !struct_generics.is_empty()
            );

        if is_cross_specialization {
            let struct_generic_names: Vec<String> = match module
                .definitions
                .get(receiver_qualified_name)
                .map(|d| &d.body)
            {
                Some(DefinitionBody::Struct { generics: g, .. }) => {
                    g.iter().map(|g| g.name.to_string()).collect()
                }
                _ => vec![],
            };
            self.sink.push(
                diagnostics::infer::duplicate_method_across_specialized_impls(
                    fn_name,
                    type_name,
                    &struct_generic_names,
                    fn_name_span,
                ),
            );
        } else {
            self.sink.push(diagnostics::infer::duplicate_impl_item(
                fn_name,
                type_name,
                fn_name_span,
            ));
        }
    }

    pub(super) fn populate_impl_methods(
        &mut self,
        store: &mut Store,
        annotation: &Annotation,
        generics: &[Generic],
        functions: &[Expression],
        span: &Span,
    ) {
        self.scopes.push();
        self.put_in_scope(generics);

        self.check_undeclared_impl_type_params(annotation, generics);
        let receiver_ty = self.convert_to_type(&*store, annotation, span);
        let Some(type_name) = receiver_ty.get_name() else {
            self.scopes.pop();
            return;
        };
        let receiver_qualified_name = receiver_ty.get_qualified_name();
        let module_id = self.cursor.module_id.clone();

        if let Some(type_module) = store.module_for_qualified_name(&receiver_qualified_name)
            && type_module != module_id
        {
            self.sink.push(diagnostics::infer::impl_on_foreign_type(
                type_name,
                type_module,
                *span,
            ));
            self.scopes.pop();
            return;
        }

        if !self.is_d_lis(&*store)
            && let Some(module) = store.get_module(&module_id)
            && matches!(
                module
                    .definitions
                    .get(&receiver_qualified_name)
                    .map(|d| &d.body),
                Some(DefinitionBody::TypeAlias { .. })
            )
        {
            self.sink.push(diagnostics::infer::impl_on_type_alias(
                type_name,
                annotation.get_span(),
            ));
            self.scopes.pop();
            return;
        }

        let mut impl_bounds: Vec<syntax::types::Bound> = Vec::new();
        for g in generics {
            for b in &g.bounds {
                let bound_ty = self.register_bound_annotation(&*store, b, span);
                impl_bounds.push(syntax::types::Bound {
                    param_name: g.name.clone(),
                    generic: Type::Parameter(g.name.clone()),
                    ty: bound_ty,
                });
            }
        }

        let mut static_methods: Vec<(String, Type)> = Vec::new();

        for function in functions {
            let fn_attrs = if let Expression::Function { attributes, .. } = function {
                attributes.as_slice()
            } else {
                &[]
            };
            let fn_visibility = if let Expression::Function { visibility, .. } = function
                && (*visibility == SyntacticVisibility::Public || self.is_d_lis(&*store))
            {
                Visibility::Public
            } else {
                Visibility::Private
            };
            let fn_sig = function.to_function_signature();
            let mut fn_ty = self.extract_function_signature(&*store, &fn_sig, span);
            let qualified_name = format!("{}.{}", type_name, fn_sig.name);
            let module_qualified_name = Symbol::from_parts(&module_id, &qualified_name);
            let is_instance_method = fn_sig.params.first().is_some_and(|p| {
                matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
            });

            let has_unannotated_self = fn_sig
                .params
                .first()
                .is_some_and(|p| p.annotation.is_none());

            if is_instance_method && has_unannotated_self {
                fn_ty = fn_ty.with_replaced_first_param(&receiver_ty);
            }

            let method_ty = wrap_with_impl_generics(&fn_ty, generics, &impl_bounds);

            if !generics.is_empty()
                && self.impl_has_simple_type_params(&receiver_ty, generics)
                && has_recursive_instantiation(&receiver_qualified_name, &fn_ty)
            {
                self.sink
                    .push(diagnostics::infer::recursive_generic_instantiation(
                        type_name,
                        fn_sig.name_span,
                    ));
            }

            if is_instance_method {
                if !self.try_register_instance_method(
                    store,
                    &module_id,
                    &receiver_qualified_name,
                    type_name,
                    &fn_sig.name,
                    fn_sig.name_span,
                    &method_ty,
                ) {
                    continue;
                }
            } else {
                static_methods.push((qualified_name, method_ty.clone()));
            }

            self.check_duplicate_method(
                &*store,
                &module_id,
                &receiver_qualified_name,
                type_name,
                &fn_sig.name,
                fn_sig.name_span,
                generics.is_empty(),
            );

            let module = store
                .get_module_mut(&module_id)
                .expect("current module must exist in store");
            module.definitions.insert(
                module_qualified_name,
                Definition {
                    visibility: fn_visibility.clone(),
                    ty: method_ty,
                    name: None,
                    name_span: Some(fn_sig.name_span),
                    doc: None,
                    body: DefinitionBody::Value {
                        allowed_lints: extract_attribute_flags(fn_attrs, "allow"),
                        go_hints: extract_attribute_flags(fn_attrs, "go"),
                        go_name: None,
                    },
                },
            );
        }

        self.scopes.pop();

        let scope = self.scopes.current_mut();
        for (name, ty) in static_methods {
            scope.values.insert(name, ty);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn populate_interface(
        &mut self,
        store: &mut Store,
        interface_name: &str,
        name_span: &Span,
        generics: &[Generic],
        parents: &[ParentInterface],
        fn_expressions: &[Expression],
        span: &Span,
        doc: &Option<String>,
    ) {
        self.scopes.push();
        self.put_in_scope(generics);
        self.validate_generic_bounds(&*store, generics, span);

        let new_parents = parents
            .iter()
            .map(|s| self.convert_to_type(&*store, &s.annotation, &s.span))
            .collect();

        let mut method_defs: Vec<(EcoString, Type, Vec<String>)> = Vec::new();
        let methods = fn_expressions
            .iter()
            .map(|fe| {
                let fn_attrs = if let Expression::Function { attributes, .. } = fe {
                    attributes.as_slice()
                } else {
                    &[]
                };
                let method_sig = fe.to_function_signature();
                let fn_ty = self.extract_function_signature(&*store, &method_sig, span);
                let fn_ty = match &fn_ty {
                    Type::Forall { body, .. } => body.as_ref().clone(),
                    _ => fn_ty,
                };

                // Strip bare `self` parameter from the interface method type.
                // The satisfaction checker also strips the receiver from impl methods
                // via `remove_first_param`, so both sides must omit the receiver.
                let has_self_receiver = method_sig.params.first().is_some_and(|p| {
                    matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
                        && p.annotation.is_none()
                });
                let fn_ty = if has_self_receiver {
                    match fn_ty {
                        Type::Function {
                            params,
                            param_mutability,
                            bounds,
                            return_type,
                        } => Type::Function {
                            params: params[1..].to_vec(),
                            param_mutability: if param_mutability.is_empty() {
                                vec![]
                            } else {
                                param_mutability[1..].to_vec()
                            },
                            bounds,
                            return_type,
                        },
                        other => other,
                    }
                } else {
                    fn_ty
                };

                method_defs.push((
                    method_sig.name.clone(),
                    fn_ty.clone(),
                    extract_attribute_flags(fn_attrs, "go"),
                ));
                (method_sig.name, fn_ty)
            })
            .collect();

        self.scopes.pop();

        let qualified_name = self.qualify_name(interface_name);
        let interface_ty = store
            .get_type(&qualified_name)
            .expect("interface type scheme must exist")
            .clone();

        let interface = Interface {
            name: interface_name.into(),
            generics: generics.to_owned(),
            parents: new_parents,
            methods,
        };

        let visibility = self
            .current_module(&*store)
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility().clone())
            .unwrap_or(Visibility::Private);

        let module = self.current_module_mut(store);

        module.definitions.insert(
            qualified_name.clone(),
            Definition {
                visibility: visibility.clone(),
                ty: interface_ty,
                name: None,
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Interface {
                    definition: interface,
                },
            },
        );

        // Register interface methods as Definition::Value entries so the emitter
        // can look up their go_hints (e.g., comma_ok) by qualified name.
        // Methods inherit the interface's visibility — a `pub interface`'s methods are implicitly public.
        let module_id = self.cursor.module_id.clone();
        for (method_name, method_ty, go_hints) in method_defs {
            let method_qualified_name = format!("{}.{}.{}", module_id, interface_name, method_name);
            module.definitions.insert(
                method_qualified_name.into(),
                Definition {
                    visibility: visibility.clone(),
                    ty: method_ty,
                    name: None,
                    name_span: None, // Interface method signatures; span tracked in Interface definition
                    doc: None,
                    body: DefinitionBody::Value {
                        allowed_lints: vec![],
                        go_hints,
                        go_name: None,
                    },
                },
            );
        }

        self.check_interface_embedding(&*store, &qualified_name, interface_name, name_span);
    }

    fn check_interface_embedding(
        &mut self,
        store: &Store,
        qualified_name: &str,
        interface_name: &str,
        span: &Span,
    ) {
        let interface = match store.get_interface(qualified_name) {
            Some(iface) => iface,
            None => return,
        };

        for parent_ty in &interface.parents {
            if let Some(parent_id) = parent_ty.get_qualified_id()
                && parent_id == qualified_name
            {
                self.sink.push(diagnostics::infer::interface_self_embedding(
                    interface_name,
                    *span,
                ));
                return; // Self-embedding implies a cycle, skip further checks
            }
        }

        let mut visited = rustc_hash::FxHashSet::default();
        let mut path = vec![qualified_name.to_string()];
        visited.insert(qualified_name.to_string());

        for parent_ty in &interface.parents {
            if let Some(parent_id) = parent_ty.get_qualified_id()
                && let Some(cycle) =
                    self.detect_interface_cycle(store, parent_id, &mut visited, &mut path)
            {
                self.sink
                    .push(diagnostics::infer::interface_embedding_cycle(&cycle, *span));
                return; // Found a cycle, skip method conflict checks
            }
        }

        let mut inherited_methods: Vec<(String, Type, String)> = Vec::new();
        let mut method_visited = rustc_hash::FxHashSet::default();

        for parent_ty in &interface.parents {
            if let Some(parent_id) = parent_ty.get_qualified_id() {
                let parent_name = unqualified_name(parent_id);
                self.collect_interface_methods(
                    store,
                    parent_id,
                    parent_name,
                    &mut inherited_methods,
                    &mut method_visited,
                );
            }
        }

        // Check for conflicts: same method name with different types from different sources
        let mut seen: rustc_hash::FxHashMap<String, (Type, String)> =
            rustc_hash::FxHashMap::default();
        for (method_name, method_ty, source) in &inherited_methods {
            if let Some((existing_ty, existing_source)) = seen.get(method_name) {
                if existing_ty != method_ty {
                    self.sink
                        .push(diagnostics::infer::interface_method_conflict(
                            interface_name,
                            method_name,
                            existing_source,
                            source,
                            *span,
                        ));
                }
            } else {
                seen.insert(method_name.clone(), (method_ty.clone(), source.clone()));
            }
        }
    }

    fn detect_interface_cycle(
        &self,
        store: &Store,
        current_id: &str,
        visited: &mut rustc_hash::FxHashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if !visited.insert(current_id.to_string()) {
            // Found a cycle — build the cycle path from where the repeated node appears
            let simple = |id: &str| -> String { unqualified_name(id).to_string() };
            if let Some(position) = path.iter().position(|p| p == current_id) {
                let mut cycle: Vec<String> = path[position..].iter().map(|p| simple(p)).collect();
                cycle.push(simple(current_id));
                return Some(cycle);
            }
            return None;
        }

        path.push(current_id.to_string());

        if let Some(interface) = store.get_interface(current_id) {
            for parent_ty in &interface.parents {
                if let Some(parent_id) = parent_ty.get_qualified_id()
                    && let Some(cycle) =
                        self.detect_interface_cycle(store, parent_id, visited, path)
                {
                    path.pop();
                    return Some(cycle);
                }
            }
        }

        path.pop();
        visited.remove(current_id); // Backtrack to allow other paths through this node
        None
    }

    fn collect_interface_methods(
        &self,
        store: &Store,
        interface_id: &str,
        source_name: &str,
        methods: &mut Vec<(String, Type, String)>,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) {
        if !visited.insert(interface_id.to_string()) {
            return;
        }

        if let Some(interface) = store.get_interface(interface_id) {
            for (method_name, method_ty) in &interface.methods {
                methods.push((
                    method_name.to_string(),
                    method_ty.clone(),
                    source_name.to_string(),
                ));
            }

            for parent_ty in &interface.parents {
                if let Some(parent_id) = parent_ty.get_qualified_id() {
                    let parent_simple = unqualified_name(parent_id);
                    self.collect_interface_methods(
                        store,
                        parent_id,
                        parent_simple,
                        methods,
                        visited,
                    );
                }
            }
        }
    }

    /// Check if the impl receiver type has simple type parameters that match the generics.
    /// E.g., `impl<T> Box<T>` has simple params (T maps directly to the generic T).
    /// `impl<U> Option<Option<U>>` does NOT have simple params (Option<U> is not a bare generic).
    fn impl_has_simple_type_params(&self, receiver_ty: &Type, generics: &[Generic]) -> bool {
        let params = match receiver_ty {
            Type::Nominal { params, .. } => params,
            _ => return false,
        };

        if params.len() != generics.len() {
            return false;
        }

        params
            .iter()
            .zip(generics.iter())
            .all(|(param, generic)| matches!(param, Type::Parameter(name) if *name == generic.name))
    }
}
