use crate::checker::EnvResolve;
use crate::store::Store;
use diagnostics::infer::{InterfaceViolation, MissingMethod};
use syntax::ast::Span;
use syntax::program::{DefinitionBody, Interface, MethodSignatures};
use syntax::types::{GO_IMPORT_PREFIX, SubstitutionMap, Type, substitute};

use crate::checker::infer::InferCtx;

struct PointerReceiverCheck<'a> {
    methods: &'a MethodSignatures,
    receiver: &'a Type,
    found: Vec<String>,
    visiting: rustc_hash::FxHashSet<String>,
}

struct ConformanceTraversal<'a> {
    receiver: &'a Type,
    span: Span,
    adapter_capable: bool,
    violations: Vec<InterfaceViolation>,
    visiting: rustc_hash::FxHashSet<String>,
}

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
            iface.parents.iter().any(|parent| {
                parent
                    .get_qualified_name()
                    .is_some_and(|id| walk(store, id.as_str(), method, seen))
            })
        })
    }
    walk(
        store,
        type_id,
        method,
        &mut rustc_hash::FxHashSet::default(),
    )
}

impl InferCtx<'_> {
    pub(crate) fn check_concrete_bound(&mut self, ty: &Type, bound: &Type, span: &Span) {
        let bound = self.store.deep_resolve_alias(bound);
        let Type::Nominal { id, params, .. } = bound else {
            return;
        };
        let Some(interface) = self.store.get_interface(&id).cloned() else {
            return;
        };
        if self
            .satisfies_interface(ty, &interface, &id, &params, span)
            .is_ok()
        {
            let _ = self.check_pointer_receivers(ty, &interface, &id, span);
        }
    }

    pub(super) fn satisfies_interface(
        &mut self,
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
        type_args: &[Type],
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let resolved = ty.resolve_in(&self.env);
        let (core, behind_ref) = self.store.peel_refs_and_aliases(&resolved);
        if behind_ref
            && self.store.is_interface(&core)
            && interface_declares_methods(
                self.store,
                interface,
                &mut rustc_hash::FxHashSet::default(),
            )
        {
            self.sink
                .push(diagnostics::infer::ref_to_interface_does_not_implement(
                    &interface.name,
                    &resolved,
                    *span,
                ));
            return Err(vec![]);
        }

        // Get type ID to track circular satisfaction checks.
        // If we're already checking if this type satisfies this interface, return success
        // to prevent infinite recursion (e.g., interface Fluent { fn next() -> Fluent }).
        let type_id = resolved
            .get_qualified_id()
            .map(String::from)
            .unwrap_or_else(|| ty.to_string());
        let pair = (type_id, interface_qualified_id.to_string());

        if !self.satisfying_stack.insert(pair.clone()) {
            return Ok(());
        }

        let adapter_capable = self.adapter_capable_receiver(ty, interface, interface_qualified_id);
        let mut check = ConformanceTraversal {
            receiver: ty,
            span: *span,
            adapter_capable,
            violations: Vec::new(),
            visiting: rustc_hash::FxHashSet::default(),
        };
        self.collect_interface_violations(
            &mut check,
            interface,
            interface_qualified_id,
            type_args,
            None,
        );
        let violations = check.violations;

        self.satisfying_stack.remove(&pair);

        let builtin_receiver = self.is_builtin_receiver(ty);

        if violations.is_empty()
            && !(builtin_receiver
                && interface_declares_methods(
                    self.store,
                    interface,
                    &mut rustc_hash::FxHashSet::default(),
                ))
        {
            return Ok(());
        }

        let resolved = ty.resolve_in(&self.env);
        if let Some(sealed) = violations.iter().find(|v| {
            v.missing
                .iter()
                .any(|method| crate::checker::sealing::is_unexported_key(&method.name))
        }) {
            let type_name = resolved
                .get_name()
                .map_or_else(|| resolved.to_string(), str::to_owned);
            self.sink
                .push(diagnostics::infer::sealed_interface_not_satisfiable(
                    &sealed.interface_name,
                    &type_name,
                    *span,
                ));
            return Err(violations);
        }
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
        } else if builtin_receiver {
            let type_name = resolved
                .get_name()
                .map_or_else(|| resolved.to_string(), str::to_owned);
            self.sink
                .push(diagnostics::infer::builtin_type_cannot_implement_interface(
                    &interface.name,
                    &type_name,
                    *span,
                ));
        } else {
            let type_name = resolved
                .get_name()
                .map_or_else(|| resolved.to_string(), str::to_owned);
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

    fn is_builtin_receiver(&self, ty: &Type) -> bool {
        let resolved = self
            .store
            .deep_resolve_alias(&ty.strip_refs().resolve_in(&self.env));
        match resolved.strip_refs() {
            Type::Simple(_) | Type::Compound { .. } | Type::Array { .. } => true,
            Type::Nominal { id, .. } => {
                id.as_str().starts_with("prelude.")
                    && self.store.get_interface(id.as_str()).is_none()
            }
            _ => false,
        }
    }

    /// In Go, if any method has a pointer receiver, only a pointer satisfies the
    /// interface. Runs on direct value-to-interface assignment and bounds checking,
    /// minus generics absorbed via a `Ref<T>` param (see `generic_absorbed_via_ref_param`).
    pub(super) fn check_pointer_receivers(
        &mut self,
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let store = self.store;
        if store.peel_alias(ty).is_ref() {
            return Ok(());
        }

        let methods = self.get_all_methods(store, ty);
        let mut check = PointerReceiverCheck {
            methods: &methods,
            receiver: ty,
            found: Vec::new(),
            visiting: rustc_hash::FxHashSet::default(),
        };
        self.collect_pointer_receiver_methods(&mut check, interface, interface_qualified_id);
        let ptr_methods = check.found;

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
        check: &mut PointerReceiverCheck<'_>,
        interface: &Interface,
        interface_qualified_id: &str,
    ) {
        let store = self.store;
        if !check.visiting.insert(interface_qualified_id.to_string()) {
            return;
        }
        let interface_is_public = store
            .get_definition(interface_qualified_id)
            .is_some_and(|d| d.visibility.is_public());
        for name in interface.methods.keys() {
            if let Some((impl_name, method_ty)) = syntax::go_names::conformance_method(
                check.methods,
                interface_qualified_id,
                interface_is_public,
                name.as_str(),
                &|candidate| self.conformance_candidate(check.receiver, candidate),
            ) {
                let func = match method_ty {
                    Type::Forall { body, .. } => body.as_ref(),
                    other => other,
                };
                if let Type::Function(f) = func
                    && f.params.first().is_some_and(|param| param.ty.is_ref())
                {
                    check.found.push(impl_name.to_string());
                }
            }
        }
        for parent in &interface.parents {
            let Some(parent_name) = parent.get_qualified_name() else {
                continue;
            };
            if let Some(parent_interface) = store.get_interface(&parent_name) {
                self.collect_pointer_receiver_methods(
                    check,
                    parent_interface,
                    parent_name.as_str(),
                );
            }
        }
        check.visiting.remove(interface_qualified_id);
    }

    fn conformance_candidate(
        &self,
        receiver: &Type,
        method: &str,
    ) -> syntax::go_names::ConformanceCandidate {
        let resolved = self
            .store
            .deep_resolve_alias(&receiver.strip_refs().resolve_in(&self.env));
        self.own_candidate(&resolved, method)
            .or_else(|| self.promoted_candidate(&resolved, method))
            .or_else(|| self.bound_candidate(&resolved, method))
            .unwrap_or(syntax::go_names::ConformanceCandidate::Unresolved)
    }

    fn own_candidate(
        &self,
        resolved: &Type,
        method: &str,
    ) -> Option<syntax::go_names::ConformanceCandidate> {
        let id = resolved.get_qualified_id()?;
        let public = method_definition_public(
            self.store,
            id,
            method,
            &mut rustc_hash::FxHashSet::default(),
        )?;
        // UFCS-lowered methods emit as free functions, not selectors.
        Some(syntax::go_names::ConformanceCandidate::Resolved {
            exported: public,
            depth: 0,
            owner: id.into(),
            shadowed: self.is_ufcs_method(id, method),
        })
    }

    fn promoted_candidate(
        &self,
        resolved: &Type,
        method: &str,
    ) -> Option<syntax::go_names::ConformanceCandidate> {
        use crate::checker::promotion;
        let store = self.store;
        resolved.get_qualified_id()?;
        let promotion::Resolution::Found(member) =
            promotion::resolve_selector(store, resolved, method)
        else {
            return None;
        };
        let public = method_definition_public(
            store,
            member.declaring_type.as_str(),
            method,
            &mut rustc_hash::FxHashSet::default(),
        )
        .unwrap_or(false);
        let selector = if public {
            syntax::go_names::snake_to_camel(method)
        } else {
            syntax::go_names::unexported_method_go_name(method)
        };
        let shadowed = promotion::field_selector_depth(store, resolved, &selector)
            .is_some_and(|field_depth| field_depth <= member.depth);
        Some(syntax::go_names::ConformanceCandidate::Resolved {
            exported: public,
            depth: member.depth,
            owner: member.declaring_type.as_eco().clone(),
            shadowed,
        })
    }

    // Bound method sets merge as one Go constraint interface.
    fn bound_candidate(
        &self,
        resolved: &Type,
        method: &str,
    ) -> Option<syntax::go_names::ConformanceCandidate> {
        let Type::Parameter(name) = resolved else {
            return None;
        };
        let bounds = self.scopes.collect_all_trait_bounds();
        let qualified = self.qualify_name(name);
        bounds.get(&qualified)?.iter().find_map(|bound| {
            let iface_id = self
                .store
                .deep_resolve_alias(bound)
                .get_qualified_id()?
                .to_string();
            let public = method_definition_public(
                self.store,
                &iface_id,
                method,
                &mut rustc_hash::FxHashSet::default(),
            )?;
            Some(syntax::go_names::ConformanceCandidate::Resolved {
                exported: public,
                depth: 0,
                owner: qualified.as_eco().clone(),
                shadowed: false,
            })
        })
    }

    /// Mirror of emit's `needs_adapter` precondition.
    fn adapter_capable_receiver(
        &self,
        ty: &Type,
        interface: &Interface,
        interface_qualified_id: &str,
    ) -> bool {
        let store = self.store;
        let resolved = store.deep_resolve_alias(&ty.strip_refs().resolve_in(&self.env));
        let Some(id) = resolved.get_qualified_id() else {
            return false;
        };
        if id.starts_with(GO_IMPORT_PREFIX) {
            return false;
        }
        let own = match store.get_definition(id).map(|d| &d.body) {
            Some(DefinitionBody::Struct { methods, .. })
            | Some(DefinitionBody::Enum { methods, .. }) => methods,
            _ => return false,
        };
        self.own_methods_cover_interface(
            own,
            id,
            interface,
            interface_qualified_id,
            &mut rustc_hash::FxHashSet::default(),
        )
    }

    fn own_methods_cover_interface(
        &self,
        own: &MethodSignatures,
        own_id: &str,
        interface: &Interface,
        interface_qualified_id: &str,
        seen: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        let store = self.store;
        if !seen.insert(interface_qualified_id.to_string()) {
            return true;
        }
        let interface_is_public = store
            .get_definition(interface_qualified_id)
            .is_some_and(|d| d.visibility.is_public());
        let own_candidate = |name: &str| {
            store
                .get_definition(&format!("{own_id}.{name}"))
                .map(
                    |definition| syntax::go_names::ConformanceCandidate::Resolved {
                        exported: definition.visibility.is_public(),
                        depth: 0,
                        owner: own_id.into(),
                        shadowed: self.is_ufcs_method(own_id, name),
                    },
                )
                .unwrap_or(syntax::go_names::ConformanceCandidate::Unresolved)
        };
        let covered = interface.methods.keys().all(|method| {
            syntax::go_names::conformance_method(
                own,
                interface_qualified_id,
                interface_is_public,
                method.as_str(),
                &own_candidate,
            )
            .is_some()
        });
        covered
            && interface.parents.iter().all(|parent| {
                let Some(parent_name) = parent.get_qualified_name() else {
                    return true;
                };
                match store.get_interface(&parent_name) {
                    Some(parent_interface) => self.own_methods_cover_interface(
                        own,
                        own_id,
                        parent_interface,
                        parent_name.as_str(),
                        seen,
                    ),
                    None => true,
                }
            })
    }

    fn collect_interface_violations(
        &mut self,
        check: &mut ConformanceTraversal<'_>,
        interface: &Interface,
        interface_qualified_id: &str,
        type_args: &[Type],
        parent_of: Option<&str>,
    ) {
        let store = self.store;
        if !check.visiting.insert(interface_qualified_id.to_string()) {
            return;
        }
        let ty = check.receiver;
        let span = &check.span;

        let symbol_methods = self.get_all_methods(store, ty);

        let map: SubstitutionMap = interface
            .generics
            .iter()
            .map(|g| g.name.clone())
            .zip(type_args.iter().cloned())
            .collect();

        let mut missing: Vec<MissingMethod> = Vec::new();
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

        let interface_is_public = store
            .get_definition(interface_qualified_id)
            .is_some_and(|d| d.visibility.is_public());

        for (method_name, method_ty) in &interface.methods {
            let selected = self.select_impl_method(
                ty,
                &symbol_methods,
                interface_qualified_id,
                interface_is_public,
                method_name.as_str(),
                receiver_id.as_ref().map(|id| id.as_str()),
            );
            let (impl_method_name, symbol_method) = match selected {
                SelectedMethod::Found(name, method) => (name, method),
                SelectedMethod::UfcsOnly => {
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
                    missing.push(MissingMethod {
                        name: method_name.to_string(),
                        signature: method_ty.clone(),
                        private_candidate: None,
                    });
                    continue;
                }
                SelectedMethod::Missing => {
                    let private_candidate = self.private_method_hint(
                        ty,
                        &symbol_methods,
                        interface_qualified_id,
                        method_name.as_str(),
                        method_ty,
                        &map,
                    );
                    missing.push(MissingMethod {
                        name: method_name.to_string(),
                        signature: method_ty.clone(),
                        private_candidate,
                    });
                    continue;
                }
            };

            let signature = self.check_method_signature(
                ty,
                interface_qualified_id,
                method_name.as_str(),
                method_ty,
                &symbol_method,
                &map,
            );

            if signature.receiver_pinned && signature.matched {
                self.validate_comma_ok_abi(
                    check,
                    interface,
                    interface_qualified_id,
                    method_name,
                    impl_method_name.as_str(),
                );
            }

            if !signature.receiver_pinned {
                missing.push(MissingMethod {
                    name: method_name.to_string(),
                    signature: method_ty.clone(),
                    private_candidate: None,
                });
            } else if !signature.matched {
                incompatible.push((
                    method_name.to_string(),
                    signature.substituted_method,
                    signature.incompatible_impl,
                ));
            } else {
                let spelling_pinned = syntax::go_names::interface_matches_by_source_name(
                    interface_qualified_id,
                    interface_is_public,
                );
                self.record_conformance_use(ty, impl_method_name.as_str(), spelling_pinned);
            }
        }

        if !missing.is_empty() || !incompatible.is_empty() {
            check.violations.push(InterfaceViolation {
                interface_name: interface.name.to_string(),
                parent_of: parent_of.map(String::from),
                missing,
                incompatible,
            });
        }

        for parent in &interface.parents {
            let Some(parent_name) = parent.get_qualified_name() else {
                continue;
            };
            if let Some(parent_interface) = store.get_interface(&parent_name).cloned() {
                let parent_type_args = parent.get_type_params().unwrap_or_default();
                let substituted_parent_args: Vec<Type> = parent_type_args
                    .iter()
                    .map(|arg| substitute(arg, &map))
                    .collect();
                self.collect_interface_violations(
                    check,
                    &parent_interface,
                    &parent_name,
                    &substituted_parent_args,
                    Some(&interface.name),
                );
            }
        }

        check.visiting.remove(interface_qualified_id);
    }

    fn select_impl_method(
        &self,
        ty: &Type,
        symbol_methods: &MethodSignatures,
        interface_qualified_id: &str,
        interface_is_public: bool,
        method_name: &str,
        receiver_id: Option<&str>,
    ) -> SelectedMethod {
        let selected = syntax::go_names::conformance_method(
            symbol_methods,
            interface_qualified_id,
            interface_is_public,
            method_name,
            &|name| self.conformance_candidate(ty, name),
        );
        let ufcs_probe = match &selected {
            Some((name, _)) => name.as_str(),
            None => method_name,
        };
        if receiver_id.is_some_and(|id| self.is_ufcs_method(id, ufcs_probe)) {
            return SelectedMethod::UfcsOnly;
        }
        match selected {
            Some((name, method)) => SelectedMethod::Found(name.clone(), method.clone()),
            None => SelectedMethod::Missing,
        }
    }

    fn private_method_hint(
        &mut self,
        ty: &Type,
        symbol_methods: &MethodSignatures,
        interface_qualified_id: &str,
        method_name: &str,
        method_ty: &Type,
        map: &SubstitutionMap,
    ) -> Option<String> {
        let interface_is_public = self
            .store
            .get_definition(interface_qualified_id)
            .is_some_and(|d| d.visibility.is_public());
        let (impl_name, impl_method) = syntax::go_names::conformance_method_if_public(
            symbol_methods,
            interface_qualified_id,
            interface_is_public,
            method_name,
            &|name| self.conformance_candidate(ty, name),
        )?;
        let impl_name = impl_name.to_string();
        let impl_method = impl_method.clone();
        let signature = self.check_method_signature(
            ty,
            interface_qualified_id,
            method_name,
            method_ty,
            &impl_method,
            map,
        );
        (signature.receiver_pinned && signature.matched).then_some(impl_name)
    }

    fn check_method_signature(
        &mut self,
        ty: &Type,
        interface_qualified_id: &str,
        method_name: &str,
        method_ty: &Type,
        symbol_method: &Type,
        map: &SubstitutionMap,
    ) -> SignatureCheck {
        let store = self.store;
        let substituted_method = substitute(method_ty, map);

        let instantiated_method = match symbol_method {
            Type::Forall { .. } => self.instantiate(symbol_method).0,
            _ => symbol_method.clone(),
        };
        let receiver_to_pin = match symbol_method {
            Type::Forall { .. } => match &instantiated_method {
                Type::Function(f) => f
                    .params
                    .first()
                    .map(|param| &param.ty)
                    .filter(|ty| !ty.is_receiver_placeholder())
                    .map(Type::strip_refs),
                _ => None,
            },
            _ => None,
        };
        let impl_method_without_receiver = Self::remove_first_param(&instantiated_method);

        let strip_bounds = |ty: &Type| match ty {
            Type::Function(f) => f.rebuild(f.params.clone(), vec![], f.return_type.clone()),
            other => other.clone(),
        };

        let impl_for_unify = covariant_return_adjustment(
            interface_qualified_id,
            method_name,
            &substituted_method,
            &impl_method_without_receiver,
            store,
        )
        .unwrap_or_else(|| impl_method_without_receiver.clone());

        let candidate_ty = ty.strip_refs().resolve_in(&self.env);
        let mut receiver_pinned = true;
        let mut resolved_impl_method = None;
        self.scopes.enter_invariant_position();
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
        self.scopes.exit_invariant_position();

        SignatureCheck {
            receiver_pinned,
            matched: sig_match.is_ok(),
            substituted_method,
            incompatible_impl: resolved_impl_method.unwrap_or(impl_method_without_receiver),
        }
    }

    fn validate_comma_ok_abi(
        &mut self,
        check: &ConformanceTraversal<'_>,
        interface: &Interface,
        interface_qualified_id: &str,
        method_name: &str,
        impl_method_name: &str,
    ) {
        let store = self.store;
        let resolved_ty = check.receiver.strip_refs().resolve_in(&self.env);
        if resolved_ty.get_qualified_id().is_none() {
            return;
        }
        let crate::checker::promotion::Resolution::Found(member) =
            crate::checker::promotion::resolve_selector(store, &resolved_ty, impl_method_name)
        else {
            return;
        };
        let interface_comma_ok = method_comma_ok(store, interface_qualified_id, method_name);
        let selected_comma_ok =
            method_comma_ok(store, member.declaring_type.as_str(), impl_method_name);
        let adapter_reconciles = check.adapter_capable && interface_comma_ok && !selected_comma_ok;
        if interface_comma_ok != selected_comma_ok && !adapter_reconciles {
            self.sink.push(diagnostics::embed::comma_ok_abi_mismatch(
                &interface.name,
                method_name,
                check.span,
            ));
        }
    }

    fn record_conformance_use(&mut self, ty: &Type, impl_method_name: &str, spelling_pinned: bool) {
        let store = self.store;
        if let Type::Nominal { id, .. } = ty.strip_refs().resolve_in(&self.env)
            && let Some(module) = store.module_for_qualified_name(id.as_str())
            && let Some(type_name) = id.as_str().get(module.len() + 1..)
            && !type_name.contains('.')
        {
            self.facts.mark_method_used_for_interface(
                module.to_string(),
                impl_method_name.to_string(),
                type_name.to_string(),
                spelling_pinned,
            );
        }
    }

    fn remove_first_param(ty: &Type) -> Type {
        match ty {
            Type::Function(f) => f.without_receiver(),
            _ => ty.clone(),
        }
    }
}

enum SelectedMethod {
    Found(syntax::EcoString, Type),
    UfcsOnly,
    Missing,
}

struct SignatureCheck {
    receiver_pinned: bool,
    matched: bool,
    substituted_method: Type,
    incompatible_impl: Type,
}

fn method_definition_public(
    store: &Store,
    owner: &str,
    method: &str,
    seen: &mut rustc_hash::FxHashSet<String>,
) -> Option<bool> {
    if !seen.insert(owner.to_string()) {
        return None;
    }
    if let Some(def) = store.get_definition(&format!("{owner}.{method}")) {
        return Some(def.visibility.is_public());
    }
    let interface = store.get_interface(owner)?;
    interface.parents.iter().find_map(|parent| {
        method_definition_public(store, parent.get_qualified_name()?.as_str(), method, seen)
    })
}

pub(crate) fn interface_requires_methods(store: &Store, id: &str) -> bool {
    store.get_interface(id).is_some_and(|interface| {
        interface_declares_methods(store, interface, &mut rustc_hash::FxHashSet::default())
    })
}

fn interface_declares_methods(
    store: &Store,
    interface: &Interface,
    seen: &mut rustc_hash::FxHashSet<String>,
) -> bool {
    if !interface.methods.is_empty() {
        return true;
    }
    interface.parents.iter().any(|parent| {
        parent.get_qualified_name().is_some_and(|parent_name| {
            seen.insert(parent_name.to_string())
                && store
                    .get_interface(&parent_name)
                    .is_some_and(|parent_interface| {
                        interface_declares_methods(store, parent_interface, seen)
                    })
        })
    })
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

    Some(impl_f.rebuild(
        impl_f.params.clone(),
        impl_f.bounds.clone(),
        iface_ret.clone(),
    ))
}
