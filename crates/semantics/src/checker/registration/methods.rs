use ecow::EcoString;
use syntax::ast::{
    Annotation, Expression, Generic, Pattern, Span, Visibility as SyntacticVisibility,
};
use syntax::program::{
    Definition, DefinitionBody, Interface, Method, Visibility, interface_requirements,
};
use syntax::types::{Bound, Symbol, Type, type_args_match_params, unqualified_name};

use super::{extract_attribute_flags, has_recursive_instantiation, wrap_with_impl_generics};
use crate::checker::{TaskState, resolved_generic_bounds};
use crate::store::Store;

struct ImplReceiver<'a> {
    package_id: &'a str,
    qualified_name: &'a str,
    display_name: &'a str,
    receiver_ty: &'a Type,
}

struct MethodName<'a> {
    source: &'a str,
    key: &'a str,
}

/// Receiver type resolved from an `impl` block's annotation, with its generics' bounds registered.
struct ResolvedImplReceiver {
    receiver_ty: Type,
    qualified_name: Symbol,
    package_id: String,
    generics: Vec<Generic>,
    impl_bounds: Vec<Bound>,
}

impl TaskState {
    /// Register an instance method on the receiver type's definition.
    /// Returns `false` if the receiver was not found (caller should skip).
    fn try_register_instance_method(
        &mut self,
        store: &mut Store,
        receiver: &ImplReceiver<'_>,
        fn_name: &str,
        fn_name_span: Span,
        method: Method,
    ) -> bool {
        let package = store
            .get_package_mut(receiver.package_id)
            .expect("current package must exist in store");

        let Some(definition) = package.definitions.get_mut(receiver.qualified_name) else {
            // Receiver type not found in current package (e.g. resolved
            // to a same-named type in another package). Skip registering
            // the method to avoid false duplicate errors.
            return false;
        };

        if let DefinitionBody::Struct { fields, .. } = &definition.body
            && fields.iter().any(|f| f.name == fn_name)
        {
            self.sink.push(diagnostics::infer::method_shadows_field(
                receiver.display_name,
                fn_name,
                fn_name_span,
            ));
        }

        if let DefinitionBody::Enum { variants, .. } = &definition.body {
            for variant in variants {
                if variant.fields.is_struct() && variant.fields.iter().any(|f| f.name == fn_name) {
                    self.sink.push(diagnostics::infer::method_shadows_field(
                        receiver.display_name,
                        fn_name,
                        fn_name_span,
                    ));
                    break;
                }
            }
        }

        if let Some(methods) = definition.methods_mut() {
            methods.insert(fn_name.into(), method);
        }

        true
    }

    fn check_duplicate_method(
        &self,
        store: &Store,
        receiver: &ImplReceiver<'_>,
        name: MethodName<'_>,
        fn_name_span: Span,
        impl_generics_empty: bool,
    ) {
        let package_qualified_name = Symbol::from_parts(receiver.package_id, receiver.display_name)
            .with_segment(name.source);

        let package = store
            .get_package(receiver.package_id)
            .expect("current package must exist in store");

        let qualified_exists = package
            .definitions
            .contains_key(package_qualified_name.as_str());
        let instance_exists = package
            .definitions
            .get(receiver.qualified_name)
            .and_then(Definition::methods)
            .is_some_and(|methods| methods.contains_key(name.key));
        if !(qualified_exists || instance_exists) {
            return;
        }

        let is_cross_specialization = impl_generics_empty
            && matches!(
                package.definitions.get(receiver.qualified_name).map(|d| &d.body),
                Some(DefinitionBody::Struct { generics: struct_generics, .. })
                    if !struct_generics.is_empty()
            );

        if is_cross_specialization {
            let struct_generic_names: Vec<String> = match package
                .definitions
                .get(receiver.qualified_name)
                .map(|d| &d.body)
            {
                Some(DefinitionBody::Struct { generics: g, .. }) => {
                    g.iter().map(|g| g.name.to_string()).collect()
                }
                _ => vec![],
            };
            self.sink.push(
                diagnostics::infer::duplicate_method_across_specialized_impls(
                    name.source,
                    receiver.display_name,
                    &struct_generic_names,
                    fn_name_span,
                ),
            );
        } else {
            self.sink.push(diagnostics::infer::duplicate_impl_item(
                name.source,
                receiver.display_name,
                fn_name_span,
            ));
        }
    }

    pub(super) fn populate_impl_methods(
        &mut self,
        store: &mut Store,
        annotation: &Annotation,
        generics: &mut [Generic],
        functions: &mut [Expression],
        span: &Span,
    ) {
        let static_methods = self.with_scope(|this| {
            this.populate_impl_methods_in_scope(store, annotation, generics, functions, span)
        });

        let scope = self.scopes.current_mut();
        for (name, ty) in static_methods {
            scope.insert_value(name, ty);
        }
    }

    fn populate_impl_methods_in_scope(
        &mut self,
        store: &mut Store,
        annotation: &Annotation,
        generics: &mut [Generic],
        functions: &mut [Expression],
        span: &Span,
    ) -> Vec<(String, Type)> {
        let Some(resolved) = self.resolve_impl_receiver(store, annotation, generics, span) else {
            return Vec::new();
        };
        let type_name = resolved
            .receiver_ty
            .get_name()
            .expect("a resolved receiver always has a name");
        let receiver = ImplReceiver {
            package_id: &resolved.package_id,
            qualified_name: resolved.qualified_name.as_str(),
            display_name: type_name,
            receiver_ty: &resolved.receiver_ty,
        };

        // Static methods land in the parent scope, since this impl's generics scope drops here.
        let mut static_methods: Vec<(String, Type)> = Vec::new();
        for function in functions {
            if let Some(entry) = self.register_impl_method(
                store,
                &receiver,
                function,
                &resolved.generics,
                &resolved.impl_bounds,
            ) {
                static_methods.push(entry);
            }
        }

        static_methods
    }

    /// Resolve an `impl` block's receiver annotation to a type, or `None` if it should be skipped.
    fn resolve_impl_receiver(
        &mut self,
        store: &mut Store,
        annotation: &Annotation,
        generics: &mut [Generic],
        span: &Span,
    ) -> Option<ResolvedImplReceiver> {
        self.put_in_scope(generics);
        self.resolve_generic_bounds(&*store, generics, span);
        let impl_bounds = resolved_generic_bounds(generics);

        self.check_undeclared_impl_type_params(annotation, generics);
        let receiver_ty = self.convert_receiver_to_type(&*store, annotation, span);
        let type_name = receiver_ty.get_name()?;
        // Prelude built-ins like `Array` have no qualified name to key methods by.
        let Some(receiver_qualified_name) = receiver_ty.get_qualified_name() else {
            self.sink.push(diagnostics::infer::impl_on_foreign_type(
                type_name,
                crate::prelude::PRELUDE_PACKAGE_ID,
                *span,
            ));
            return None;
        };
        let package_id = self.cursor.package_id().to_string();
        let is_d_lis = self.is_d_lis(&*store);

        if !is_d_lis
            && let Some(type_package) = store.package_for_qualified_name(&receiver_qualified_name)
            && type_package != package_id
        {
            self.sink.push(diagnostics::infer::impl_on_foreign_type(
                type_name,
                crate::loader::import_display_name(type_package),
                *span,
            ));
            return None;
        }

        if self.current_file_is_test(store)
            && let Some(package) = store.get_package(&package_id)
            && let Some(definition) = package.definitions.get(&receiver_qualified_name)
            && !store.is_test_definition(definition)
        {
            self.sink
                .push(diagnostics::infer::test_impl_on_production_type(
                    type_name,
                    annotation.get_span(),
                ));
            return None;
        }

        if !self.is_d_lis(&*store)
            && let Some(package) = store.get_package(&package_id)
            && matches!(
                package
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
            return None;
        }

        if self.impl_has_simple_type_params(&receiver_ty, generics) {
            let receiver_bounds =
                self.register_receiver_type_bounds(&*store, &receiver_qualified_name, generics);
            self.check_strengthened_impl_bounds(
                &*store,
                &receiver_qualified_name,
                generics,
                &impl_bounds,
                &receiver_bounds,
            );
        }
        self.check_transitive_generic_bounds(&*store, generics, *span);

        Some(ResolvedImplReceiver {
            receiver_ty,
            qualified_name: receiver_qualified_name,
            package_id,
            generics: generics.to_vec(),
            impl_bounds,
        })
    }

    /// Register one impl function as a method, returning the static-method entry (if any) for the caller.
    fn register_impl_method(
        &mut self,
        store: &mut Store,
        receiver: &ImplReceiver<'_>,
        function: &mut Expression,
        generics: &[Generic],
        impl_bounds: &[Bound],
    ) -> Option<(String, Type)> {
        let is_d_lis = self.is_d_lis(&*store);
        let Expression::Function {
            attributes: fn_attrs,
            doc: fn_doc,
            visibility,
            name: fn_name,
            name_span: fn_name_span,
            generics: fn_generics,
            params: fn_params,
            return_annotation,
            span: fn_span,
            ..
        } = function
        else {
            unreachable!("impl item must be a function")
        };
        let fn_doc = fn_doc.clone();
        let fn_visibility = if *visibility == SyntacticVisibility::Public || is_d_lis {
            Visibility::Public
        } else {
            Visibility::Private
        };
        let mut fn_ty = self.extract_signature_parts(
            &*store,
            fn_generics,
            fn_params,
            return_annotation,
            fn_span,
        );
        let qualified_name = format!("{}.{}", receiver.display_name, fn_name);
        let package_qualified_name = Symbol::from_parts(receiver.package_id, &qualified_name);
        let is_instance_method = fn_params.first().is_some_and(|p| {
            matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
        });

        let has_unannotated_self = fn_params.first().is_some_and(|p| p.annotation.is_none());

        if is_instance_method && has_unannotated_self {
            fn_ty = fn_ty.with_replaced_first_param(receiver.receiver_ty);
        }

        let (method_signature_pairs, method_signature_bounds) =
            super::function_signature_pairs(&fn_ty, fn_params, *fn_span);
        self.with_scope(|this| {
            this.put_in_scope(fn_generics);
            for bound in &method_signature_bounds {
                this.record_generic_bound(&bound.param_name, bound.ty.clone());
            }
            this.check_value_position_bounds(&*store, &[], &method_signature_pairs);
        });

        let method_ty = wrap_with_impl_generics(&fn_ty, generics, impl_bounds);

        let go_hints = extract_attribute_flags(fn_attrs, "go");
        let method_key: EcoString =
            if is_instance_method && go_hints.iter().any(|h| h == "unexported") {
                super::seal_method_key(is_d_lis, fn_attrs, receiver.package_id, fn_name)
            } else {
                fn_name.clone()
            };

        if !generics.is_empty()
            && self.impl_has_simple_type_params(receiver.receiver_ty, generics)
            && has_recursive_instantiation(receiver.qualified_name, &fn_ty)
        {
            self.sink
                .push(diagnostics::infer::recursive_generic_instantiation(
                    receiver.display_name,
                    *fn_name_span,
                ));
        }

        self.check_duplicate_method(
            &*store,
            receiver,
            MethodName {
                source: fn_name,
                key: &method_key,
            },
            *fn_name_span,
            generics.is_empty(),
        );

        let mut static_entry = None;
        if is_instance_method {
            let method = Method {
                source_name: fn_name.clone(),
                ty: method_ty.clone(),
                visibility: fn_visibility.clone(),
                name_span: Some(*fn_name_span),
                doc: fn_doc.clone(),
                allowed_lints: extract_attribute_flags(fn_attrs, "allow"),
                go_hints: go_hints.clone(),
            };
            if !self.try_register_instance_method(
                store,
                receiver,
                &method_key,
                *fn_name_span,
                method,
            ) {
                // Receiver not found: skip the duplicate check and the definition insert below.
                return None;
            }
        } else {
            static_entry = Some((qualified_name, method_ty.clone()));
        }

        if !is_instance_method {
            let package = store
                .get_package_mut(receiver.package_id)
                .expect("current package must exist in store");
            package.definitions.insert(
                package_qualified_name,
                Definition {
                    visibility: fn_visibility.clone(),
                    ty: method_ty,
                    name_span: Some(*fn_name_span),
                    doc: fn_doc,
                    body: DefinitionBody::Value {
                        kind: syntax::program::ValueKind::Runtime,
                        allowed_lints: extract_attribute_flags(fn_attrs, "allow"),
                        go_hints,
                        go_name: None,
                        go_type_param_recipe: None,
                    },
                },
            );
        }

        static_entry
    }

    pub(super) fn populate_interface(&mut self, store: &mut Store, expression: &mut Expression) {
        let Expression::Interface {
            name: interface_name,
            name_span,
            generics,
            parents,
            method_signatures: fn_expressions,
            span,
            doc,
            ..
        } = expression
        else {
            unreachable!("populate_interface called with non-Interface expression");
        };
        let package_id = self.cursor.package_id().to_string();
        let is_d_lis = self.is_d_lis(&*store);
        let qualified_name = self.qualify_name(interface_name);
        let visibility = self
            .current_package(&*store)
            .definitions
            .get(qualified_name.as_str())
            .map(|definition| definition.visibility.clone())
            .unwrap_or(Visibility::Private);
        let (generics, new_parents, methods) = self.with_scope(|this| {
            this.put_in_scope(generics);
            this.resolve_generic_bounds(&*store, generics, span);

            let new_parents = parents
                .iter()
                .map(|parent| this.convert_to_type(&*store, &parent.annotation, &parent.span))
                .collect();

            let mut self_receiver_spans = Vec::new();
            let methods = fn_expressions
                .iter_mut()
                .map(|fe| {
                    let Expression::Function {
                        attributes: fn_attrs,
                        doc: fn_doc,
                        name: method_name,
                        name_span: method_name_span,
                        generics: method_generics,
                        params: method_params,
                        return_annotation,
                        span: method_span,
                        ..
                    } = fe
                    else {
                        unreachable!("interface item must be a function signature")
                    };
                    let fn_doc = fn_doc.clone();
                    let fn_ty = this.extract_signature_parts(
                        &*store,
                        method_generics,
                        method_params,
                        return_annotation,
                        method_span,
                    );
                    let fn_ty = match &fn_ty {
                        Type::Forall { body, .. } => body.as_ref().clone(),
                        _ => fn_ty,
                    };

                    let self_receiver_span = method_params.first().and_then(|p| match &p.pattern {
                        Pattern::Identifier { identifier, span } if identifier == "self" => {
                            Some(*span)
                        }
                        _ => None,
                    });
                    if let Some(self_span) = self_receiver_span {
                        self_receiver_spans.push(self_span);
                    }
                    let fn_ty = if self_receiver_span.is_some() {
                        match fn_ty {
                            Type::Function(f) => f.without_receiver(),
                            other => other,
                        }
                    } else {
                        fn_ty
                    };

                    let (mut signature_pairs, signature_bounds) =
                        super::function_signature_pairs(&fn_ty, &[], *method_span);
                    if let Type::Function(f) = fn_ty.unwrap_forall() {
                        signature_pairs.push(((*f.return_type).clone(), *method_span));
                    }
                    this.with_scope(|this| {
                        this.put_in_scope(generics);
                        this.record_resolved_generic_bounds(generics);
                        this.put_in_scope(method_generics);
                        for bound in &signature_bounds {
                            this.record_generic_bound(&bound.param_name, bound.ty.clone());
                        }
                        this.check_value_position_bounds(&*store, &[], &signature_pairs);
                    });

                    let go_hints = extract_attribute_flags(fn_attrs, "go");
                    let key = if go_hints.iter().any(|h| h == "unexported") {
                        super::seal_method_key(is_d_lis, fn_attrs, &package_id, method_name)
                    } else {
                        method_name.clone()
                    };
                    (
                        key,
                        Method {
                            source_name: method_name.clone(),
                            ty: fn_ty,
                            visibility: visibility.clone(),
                            name_span: Some(*method_name_span),
                            doc: fn_doc,
                            allowed_lints: extract_attribute_flags(fn_attrs, "allow"),
                            go_hints,
                        },
                    )
                })
                .collect();

            for self_span in self_receiver_spans {
                this.sink
                    .push(diagnostics::infer::self_in_interface_method(self_span));
            }
            (generics.clone(), new_parents, methods)
        });

        let interface_ty = store
            .get_type(&qualified_name)
            .expect("interface type scheme must exist")
            .clone();

        let interface = Interface {
            generics,
            parents: new_parents,
            methods,
        };

        let package = self.current_package_mut(store);

        package.definitions.insert(
            qualified_name.clone(),
            Definition {
                visibility: visibility.clone(),
                ty: interface_ty,
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Interface {
                    definition: interface,
                },
            },
        );

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

        let mut seen: rustc_hash::FxHashMap<String, (Type, String)> =
            rustc_hash::FxHashMap::default();
        let interface_ty = store
            .get_type(qualified_name)
            .expect("registered interface must have a type");
        for requirement in interface_requirements(interface_ty, |id| store.get_definition(id)) {
            let source = unqualified_name(&requirement.declaring_interface);
            if let Some((existing_ty, existing_source)) = seen.get(requirement.name.as_str()) {
                if existing_ty != &requirement.ty {
                    self.sink
                        .push(diagnostics::infer::interface_method_conflict(
                            interface_name,
                            &requirement.name,
                            existing_source,
                            source,
                            *span,
                        ));
                }
            } else {
                seen.insert(
                    requirement.name.to_string(),
                    (requirement.ty, source.to_string()),
                );
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
            // Found a cycle, build the cycle path from where the repeated node appears
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

    /// Check if the impl receiver type has simple type parameters that match the generics.
    /// E.g., `impl<T> Box<T>` has simple params (T maps directly to the generic T).
    /// `impl<U> Option<Option<U>>` does NOT have simple params (Option<U> is not a bare generic).
    pub(crate) fn impl_has_simple_type_params(
        &self,
        receiver_ty: &Type,
        generics: &[Generic],
    ) -> bool {
        let params = match receiver_ty {
            Type::Nominal { params, .. } => params,
            _ => return false,
        };

        type_args_match_params(params, generics.iter().map(|generic| &generic.name))
    }
}
