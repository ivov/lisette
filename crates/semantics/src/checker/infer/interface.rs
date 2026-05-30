use crate::checker::EnvResolve;
use crate::store::Store;
use diagnostics::infer::InterfaceViolation;
use syntax::ast::Span;
use syntax::program::{DefinitionBody, Interface, MethodSignatures};
use syntax::types::{GO_IMPORT_PREFIX, SubstitutionMap, Type, substitute};

use super::super::TaskState;

impl TaskState<'_> {
    pub(super) fn satisfies_interface(
        &mut self,
        store: &Store,
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
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
        let pair = (type_id, interface_qualified_id.to_string());

        if !self.satisfying_stack.insert(pair.clone()) {
            return Ok(());
        }

        let mut violations = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();
        self.collect_interface_violations(
            store,
            ty,
            interface,
            interface_qualified_id,
            type_args,
            None,
            span,
            &mut violations,
            &mut visited,
        );

        self.satisfying_stack.remove(&pair);

        if violations.is_empty() {
            Ok(())
        } else {
            let resolved = ty.resolve_in(&self.env);
            let wrapper = if resolved.is_result() {
                Some(diagnostics::infer::WrapperKind::Result)
            } else if resolved.is_option() {
                Some(diagnostics::infer::WrapperKind::Option)
            } else if resolved.is_partial() {
                Some(diagnostics::infer::WrapperKind::Partial)
            } else {
                None
            };
            if let Some(wrapper) = wrapper {
                self.sink
                    .push(diagnostics::infer::wrapper_does_not_implement_interface(
                        &interface.name,
                        wrapper,
                        &resolved,
                        *span,
                    ));
            } else {
                let type_name = ty.get_name().map_or_else(|| ty.to_string(), str::to_owned);
                self.sink
                    .push(diagnostics::infer::interface_not_implemented(
                        &interface.name,
                        &type_name,
                        &violations,
                        *span,
                    ));
            }
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
        interface_qualified_id: &str,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        if ty.is_ref() {
            return Ok(());
        }

        let methods = self.get_all_methods(store, ty);
        let mut ptr_methods = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();

        self.collect_pointer_receiver_methods(
            store,
            interface,
            interface_qualified_id,
            &methods,
            &mut ptr_methods,
            &mut visited,
        );

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
        interface_qualified_id: &str,
        methods: &MethodSignatures,
        out: &mut Vec<String>,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) {
        if !visited.insert(interface_qualified_id.to_string()) {
            return;
        }
        for name in interface.methods.keys() {
            if let Some(method_ty) = methods.get(name) {
                let func = match method_ty {
                    Type::Forall { body, .. } => body.as_ref(),
                    other => other,
                };
                if let Type::Function(f) = func
                    && f.params.first().is_some_and(|p| p.is_ref())
                {
                    out.push(name.to_string());
                }
            }
        }
        for parent in &interface.parents {
            let parent_name = parent.get_qualified_name();
            if let Some(parent_interface) = store.get_interface(&parent_name) {
                self.collect_pointer_receiver_methods(
                    store,
                    parent_interface,
                    parent_name.as_str(),
                    methods,
                    out,
                    visited,
                );
            }
        }
        visited.remove(interface_qualified_id);
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_interface_violations(
        &mut self,
        store: &Store,
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
        type_args: &[Type],
        parent_of: Option<&str>,
        span: &Span,
        violations: &mut Vec<InterfaceViolation>,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) {
        if !visited.insert(interface_qualified_id.to_string()) {
            return;
        }

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
                Type::Function(f) => Type::function(
                    f.params.clone(),
                    f.param_mutability.clone(),
                    vec![],
                    f.return_type.clone(),
                ),
                other => other.clone(),
            };

            // Go-imported interfaces allow narrow covariance: impl returning T
            // satisfies Option<T> in the top-level return position, when both
            // sides lower to the same Go ABI shape.
            let impl_for_unify = covariant_return_adjustment(
                interface_qualified_id,
                method_name.as_str(),
                &substituted_method,
                &impl_method_without_receiver,
                store,
            )
            .unwrap_or_else(|| impl_method_without_receiver.clone());

            self.scopes.increment_type_param_depth();
            let sig_match = self.speculatively(|this| {
                this.try_unify(
                    store,
                    &strip_bounds(&substituted_method),
                    &strip_bounds(&impl_for_unify),
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
            } else if let Type::Nominal { id, .. } = ty.strip_refs().resolve_in(&self.env)
                && let Some(module) = store.module_for_qualified_name(id.as_str())
                && id
                    .as_str()
                    .get(module.len() + 1..)
                    .is_some_and(|rest| !rest.contains('.'))
            {
                self.facts.mark_method_used_for_interface(
                    module.to_string(),
                    method_name.to_string(),
                    Span::dummy(),
                );
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
                    &parent_name,
                    &substituted_parent_args,
                    Some(&interface.name),
                    span,
                    violations,
                    visited,
                );
            }
        }

        visited.remove(interface_qualified_id);
    }

    fn remove_first_param(ty: &Type) -> Type {
        match ty {
            Type::Function(f) => {
                let new_params = if f.params.is_empty() {
                    vec![]
                } else {
                    f.params[1..].to_vec()
                };
                let new_mutability = if f.param_mutability.is_empty() {
                    vec![]
                } else {
                    f.param_mutability[1..].to_vec()
                };
                Type::function(
                    new_params,
                    new_mutability,
                    f.bounds.clone(),
                    f.return_type.clone(),
                )
            }
            _ => ty.clone(),
        }
    }
}

/// Lift impl return T to Option<T> when the interface is Go-imported, the
/// interface return is Option<T>, and both lower to AbiShape::NullableReturn.
/// Excludes comma_ok / sentinel shapes where the Go signatures differ.
fn covariant_return_adjustment(
    interface_qualified_id: &str,
    method_name: &str,
    interface_method: &Type,
    impl_method: &Type,
    store: &Store,
) -> Option<Type> {
    if !interface_qualified_id.starts_with(GO_IMPORT_PREFIX) {
        return None;
    }

    let (Type::Function(iface_f), Type::Function(impl_f)) = (interface_method, impl_method) else {
        return None;
    };
    let iface_ret = &iface_f.return_type;
    let impl_ret = &impl_f.return_type;

    if !iface_ret.is_option() {
        return None;
    }
    let opt_inner = iface_ret.ok_type();

    if !store.is_nilable_go_type(&opt_inner) {
        return None;
    }

    let method_qualified = format!("{}.{}", interface_qualified_id, method_name);
    let hints = store
        .get_definition(&method_qualified)
        .map(|def| def.go_hints())
        .unwrap_or(&[]);
    if hints.iter().any(|h| h == "comma_ok") {
        return None;
    }

    if opt_inner != **impl_ret {
        return None;
    }

    Some(Type::function(
        impl_f.params.clone(),
        impl_f.param_mutability.clone(),
        impl_f.bounds.clone(),
        iface_ret.clone(),
    ))
}
