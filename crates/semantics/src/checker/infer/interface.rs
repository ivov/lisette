use crate::checker::EnvResolve;
use crate::store::Store;
use diagnostics::infer::InterfaceViolation;
use syntax::ast::Span;
use syntax::program::{DefinitionBody, Interface, MethodSignatures};
use syntax::types::{GO_IMPORT_PREFIX, SubstitutionMap, Type, substitute};

use crate::checker::infer::InferCtx;

fn method_comma_ok(store: &Store, type_id: &str, method: &str) -> bool {
    fn walk(
        store: &Store,
        type_id: &str,
        method: &str,
        seen: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        if let Some(def) = store.get_definition(&format!("{type_id}.{method}")) {
            return def.go_hints().iter().any(|h| h == "comma_ok");
        }
        if !seen.insert(type_id.to_string()) {
            return false;
        }
        store.get_interface(type_id).is_some_and(|iface| {
            iface
                .parents
                .iter()
                .any(|parent| walk(store, parent.get_qualified_name().as_str(), method, seen))
        })
    }
    walk(
        store,
        type_id,
        method,
        &mut rustc_hash::FxHashSet::default(),
    )
}

impl InferCtx<'_, '_> {
    pub(super) fn satisfies_interface(
        &mut self,
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
            if let Some(sealed) = violations.iter().find(|v| {
                v.missing
                    .iter()
                    .any(|(name, _)| crate::checker::sealing::is_unexported_key(name))
            }) {
                let type_name = ty.get_name().map_or_else(|| ty.to_string(), str::to_owned);
                self.sink
                    .push(diagnostics::infer::sealed_interface_not_satisfiable(
                        &sealed.interface_name,
                        &type_name,
                        *span,
                    ));
                return Err(violations);
            }
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

    /// In Go, if any method has a pointer receiver, only a pointer satisfies the
    /// interface. Runs on direct value-to-interface assignment and bounds checking,
    /// minus generics absorbed via a `Ref<T>` param (see `generic_absorbed_via_ref_param`).
    pub(super) fn check_pointer_receivers(
        &self,
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let store = self.store;
        if ty.is_ref() {
            return Ok(());
        }

        let methods = self.get_all_methods(store, ty);
        let mut ptr_methods = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();

        self.collect_pointer_receiver_methods(
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
        interface: &Interface,
        interface_qualified_id: &str,
        methods: &MethodSignatures,
        out: &mut Vec<String>,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) {
        let store = self.store;
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
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
        type_args: &[Type],
        parent_of: Option<&str>,
        span: &Span,
        violations: &mut Vec<InterfaceViolation>,
        visited: &mut rustc_hash::FxHashSet<String>,
    ) {
        let store = self.store;
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

        let resolved_receiver = store.deep_resolve_alias(&ty.strip_refs().resolve_in(&self.env));
        let receiver_id = match &resolved_receiver {
            Type::Nominal { id, .. } => Some(id.clone()),
            _ => None,
        };
        let receiver_generics: Vec<String> = receiver_id
            .as_ref()
            .and_then(|id| {
                let generics = match &store.get_definition(id)?.body {
                    DefinitionBody::Struct { generics, .. }
                    | DefinitionBody::Enum { generics, .. }
                    | DefinitionBody::TypeAlias { generics, .. } => generics,
                    _ => return None,
                };
                Some(generics.iter().map(|g| g.name.to_string()).collect())
            })
            .unwrap_or_default();

        for (method_name, method_ty) in &interface.methods {
            if method_name.as_str() == "equals" && self.is_native_container(ty) {
                missing.push((method_name.to_string(), method_ty.clone()));
                continue;
            }
            let Some(symbol_method) = symbol_methods.get(method_name.as_str()) else {
                missing.push((method_name.to_string(), method_ty.clone()));
                continue;
            };

            if let Some(ref id) = receiver_id
                && self.is_ufcs_method(id.as_str(), method_name.as_str())
            {
                if !receiver_generics.is_empty() {
                    let type_name = resolved_receiver
                        .get_name()
                        .map_or_else(|| resolved_receiver.to_string(), str::to_owned);
                    self.sink.push(
                        diagnostics::infer::specialized_impl_cannot_satisfy_interface(
                            &type_name,
                            &interface.name,
                            method_name,
                            &receiver_generics,
                            *span,
                        ),
                    );
                }
                missing.push((method_name.to_string(), method_ty.clone()));
                continue;
            }

            let substituted_method = substitute(method_ty, &map);

            // Instantiate Forall impl methods before removing receiver
            let instantiated_method = match symbol_method {
                Type::Forall { .. } => self.instantiate(symbol_method).0,
                _ => symbol_method.clone(),
            };
            let receiver_to_pin = match symbol_method {
                Type::Forall { .. } => match &instantiated_method {
                    Type::Function(f) => f
                        .params
                        .first()
                        .filter(|p| !p.is_receiver_placeholder())
                        .map(Type::strip_refs),
                    _ => None,
                },
                _ => None,
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

            let candidate_ty = ty.strip_refs().resolve_in(&self.env);
            let mut receiver_pinned = true;
            let mut resolved_impl_method = None;
            self.scopes.increment_type_param_depth();
            let sig_match = self.speculatively(|this| {
                let mut ctx = InferCtx::new(this, store);
                if let Some(receiver) = &receiver_to_pin {
                    ctx.try_unify(receiver, &candidate_ty, &Span::dummy())
                        .inspect_err(|_| receiver_pinned = false)?;
                }
                let result = ctx.try_unify(
                    &strip_bounds(&substituted_method),
                    &strip_bounds(&impl_for_unify),
                    &Span::dummy(),
                );
                if result.is_err() {
                    resolved_impl_method = Some(impl_method_without_receiver.resolve_in(&ctx.env));
                }
                result
            });
            self.scopes.decrement_type_param_depth();

            if receiver_pinned
                && sig_match.is_ok()
                && interface_qualified_id.starts_with(GO_IMPORT_PREFIX)
            {
                let resolved_ty = ty.strip_refs().resolve_in(&self.env);
                if let Some(ty_id) = resolved_ty.get_qualified_id()
                    && let crate::checker::promotion::Resolution::Found(member) =
                        crate::checker::promotion::resolve_selector(
                            store,
                            &resolved_ty,
                            method_name.as_str(),
                        )
                {
                    let interface_comma_ok =
                        method_comma_ok(store, interface_qualified_id, method_name);
                    let selected_comma_ok =
                        method_comma_ok(store, member.declaring_type.as_str(), method_name);
                    let native_direct = member.depth == 0 && !ty_id.starts_with(GO_IMPORT_PREFIX);
                    let adapter_reconciles =
                        native_direct && interface_comma_ok && !selected_comma_ok;
                    if interface_comma_ok != selected_comma_ok && !adapter_reconciles {
                        self.sink.push(diagnostics::embed::comma_ok_abi_mismatch(
                            &interface.name,
                            method_name,
                            *span,
                        ));
                    }
                }
            }

            if !receiver_pinned {
                missing.push((method_name.to_string(), method_ty.clone()));
            } else if sig_match.is_err() {
                incompatible.push((
                    method_name.to_string(),
                    substituted_method,
                    resolved_impl_method.unwrap_or(impl_method_without_receiver),
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
