use crate::checker::EnvResolve;
use crate::store::Store;
use diagnostics::infer::InterfaceViolation;
use syntax::ast::Span;
use syntax::program::{DefinitionBody, Interface, MethodSignatures};
use syntax::types::{SubstitutionMap, Type, substitute};

use super::super::TaskState;

impl TaskState<'_> {
    pub(super) fn satisfies_interface(
        &mut self,
        store: &Store,
        ty: &Type,
        interface: &Interface,
        type_args: &[Type],
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        // Get type ID to track circular satisfaction checks.
        // If we're already checking if this type satisfies this interface, return success
        // to prevent infinite recursion (e.g., interface Fluent { fn next() -> Fluent }).
        let type_id = ty
            .resolve_in(&self.env)
            .get_qualified_id()
            .map(String::from)
            .unwrap_or_else(|| ty.to_string());
        let interface_id = interface.name.to_string();
        let pair = (type_id, interface_id);

        if !self.satisfying_stack.insert(pair.clone()) {
            return Ok(());
        }

        let mut violations = Vec::new();
        self.collect_interface_violations(
            store,
            ty,
            interface,
            type_args,
            None,
            span,
            &mut violations,
        );

        self.satisfying_stack.remove(&pair);

        if violations.is_empty() {
            Ok(())
        } else {
            let type_name = ty.get_name().map_or_else(|| ty.to_string(), str::to_owned);
            self.sink
                .push(diagnostics::infer::interface_not_implemented(
                    &interface.name,
                    &type_name,
                    &violations,
                    *span,
                ));
            Err(violations)
        }
    }

    /// In Go, if any method has a pointer receiver, only a pointer to the
    /// type satisfies the interface. This check runs only from `unify_constructors`
    /// (direct value-to-interface assignment), not from bounds checking where the
    /// emitter may absorb Ref<T> into the type parameter.
    pub(super) fn check_pointer_receivers(
        &self,
        store: &Store,
        ty: &Type,
        interface: &Interface,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        if ty.is_ref() {
            return Ok(());
        }

        let methods = self.get_all_methods(store, ty);
        let mut ptr_methods = Vec::new();

        self.collect_pointer_receiver_methods(store, interface, &methods, &mut ptr_methods);

        if ptr_methods.is_empty() {
            return Ok(());
        }

        let type_name = ty.get_name().map_or_else(|| ty.to_string(), str::to_owned);
        self.sink
            .push(diagnostics::infer::pointer_receiver_interface_mismatch(
                &interface.name,
                &type_name,
                &ptr_methods,
                *span,
            ));
        Err(vec![])
    }

    fn collect_pointer_receiver_methods(
        &self,
        store: &Store,
        interface: &Interface,
        methods: &MethodSignatures,
        out: &mut Vec<String>,
    ) {
        for name in interface.methods.keys() {
            if let Some(method_ty) = methods.get(name) {
                let func = match method_ty {
                    Type::Forall { body, .. } => body.as_ref(),
                    other => other,
                };
                if let Type::Function { params, .. } = func
                    && params.first().is_some_and(|p| p.is_ref())
                {
                    out.push(name.to_string());
                }
            }
        }
        for parent in &interface.parents {
            let parent_name = parent.get_qualified_name();
            if let Some(parent_interface) = store.get_interface(&parent_name) {
                self.collect_pointer_receiver_methods(store, parent_interface, methods, out);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_interface_violations(
        &mut self,
        store: &Store,
        ty: &Type,
        interface: &Interface,
        type_args: &[Type],
        parent_of: Option<&str>,
        span: &Span,
        violations: &mut Vec<InterfaceViolation>,
    ) {
        let symbol_methods = self.get_all_methods(store, ty);

        let map: SubstitutionMap = interface
            .generics
            .iter()
            .map(|g| g.name.clone())
            .zip(type_args.iter().cloned())
            .collect();

        let mut missing: Vec<(String, Type)> = Vec::new();
        let mut incompatible: Vec<(String, Type, Type)> = Vec::new();

        let struct_generics: Option<Vec<String>> =
            if let Type::Nominal { id, .. } = ty.strip_refs().resolve_in(&self.env) {
                store
                    .get_definition(&id)
                    .and_then(|definition| match &definition.body {
                        DefinitionBody::Struct { generics, .. } if !generics.is_empty() => {
                            Some(generics.iter().map(|g| g.name.to_string()).collect())
                        }
                        _ => None,
                    })
            } else {
                None
            };

        for (method_name, method_ty) in &interface.methods {
            let Some(symbol_method) = symbol_methods.get(method_name.as_str()) else {
                missing.push((method_name.to_string(), method_ty.clone()));
                continue;
            };

            // A method on a generic struct that is NOT wrapped in Forall came from a
            // specialized impl block. The emitter emits these as UFCS (standalone functions)
            // because Go's receiver syntax shadows type parameter names. UFCS methods cannot
            // satisfy Go interfaces, so reject them here.
            if let Some(ref generics) = struct_generics
                && !matches!(symbol_method, Type::Forall { .. })
            {
                let type_name = ty.get_name().map_or_else(|| ty.to_string(), str::to_owned);
                self.sink.push(
                    diagnostics::infer::specialized_impl_cannot_satisfy_interface(
                        &type_name,
                        &interface.name,
                        method_name,
                        generics,
                        *span,
                    ),
                );
                missing.push((method_name.to_string(), method_ty.clone()));
                continue;
            }

            let substituted_method = substitute(method_ty, &map);

            // Instantiate Forall impl methods before removing receiver
            let instantiated_method = match symbol_method {
                Type::Forall { .. } => self.instantiate(symbol_method).0,
                _ => symbol_method.clone(),
            };
            let impl_method_without_receiver = Self::remove_first_param(&instantiated_method);

            // Strip bounds before comparing - bounds are checked separately via bounds_equivalent
            let strip_bounds = |ty: &Type| match ty {
                Type::Function {
                    params,
                    param_mutability,
                    return_type,
                    ..
                } => Type::Function {
                    params: params.clone(),
                    param_mutability: param_mutability.clone(),
                    bounds: vec![],
                    return_type: return_type.clone(),
                },
                other => other.clone(),
            };

            // Require exact type matching for interface method signatures.
            // Go doesn't support covariant returns or contravariant params in interfaces.
            // Incrementing type_param_depth disables interface coercion in unify_constructors.
            // Wrap in speculatively() so that if this method's unification fails,
            // its type variable links are rolled back instead of leaking.
            self.scopes.increment_type_param_depth();
            let sig_match = self.speculatively(|this| {
                this.try_unify(
                    store,
                    &strip_bounds(&substituted_method),
                    &strip_bounds(&impl_method_without_receiver),
                    &Span::dummy(),
                )
            });
            self.scopes.decrement_type_param_depth();

            if sig_match.is_err() {
                incompatible.push((
                    method_name.to_string(),
                    substituted_method,
                    impl_method_without_receiver.clone(),
                ));
            } else if let Type::Nominal { id, .. } = ty.strip_refs().resolve_in(&self.env) {
                let parts: Vec<&str> = id.split('.').collect();
                if parts.len() == 2 {
                    self.facts.mark_method_used_for_interface(
                        parts[0].to_string(),
                        method_name.to_string(),
                        Span::dummy(),
                    );
                }
            }
        }

        if !missing.is_empty() || !incompatible.is_empty() {
            violations.push(InterfaceViolation {
                interface_name: interface.name.to_string(),
                parent_of: parent_of.map(String::from),
                missing,
                incompatible,
            });
        }

        for parent in &interface.parents {
            let parent_name = parent.get_qualified_name();
            if let Some(parent_interface) = store.get_interface(&parent_name).cloned() {
                let parent_type_args = parent.get_type_params().unwrap_or_default();
                // Substitute parent type arguments using the current interface's substitution map.
                // E.g., if Processor<T> embeds Mapper<T> and we're checking Processor<string>,
                // we need to substitute T with string before checking the embedded Mapper.
                let substituted_parent_args: Vec<Type> = parent_type_args
                    .iter()
                    .map(|arg| substitute(arg, &map))
                    .collect();
                self.collect_interface_violations(
                    store,
                    ty,
                    &parent_interface,
                    &substituted_parent_args,
                    Some(&interface.name),
                    span,
                    violations,
                );
            }
        }
    }

    fn remove_first_param(ty: &Type) -> Type {
        match ty {
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                let new_params = if params.is_empty() {
                    vec![]
                } else {
                    params[1..].to_vec()
                };
                let new_mutability = if param_mutability.is_empty() {
                    vec![]
                } else {
                    param_mutability[1..].to_vec()
                };
                Type::Function {
                    params: new_params,
                    param_mutability: new_mutability,
                    bounds: bounds.clone(),
                    return_type: return_type.clone(),
                }
            }
            _ => ty.clone(),
        }
    }
}
