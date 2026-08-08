use crate::checker::EnvResolve;
use crate::store::Store;
use diagnostics::infer::{InterfaceMethodViolation, InterfaceViolation, MissingMethod};
use syntax::ast::Span;
use syntax::program::{DefinitionBody, InterfaceRequirement, Methods, interface_requirements};
use syntax::types::{GO_IMPORT_PREFIX, Symbol, Type, unqualified_name};

use crate::checker::infer::InferCtx;

struct ConformanceTraversal<'a> {
    receiver: &'a Type,
    span: Span,
    adapter_capable: bool,
    violations: Vec<InterfaceViolation>,
}

impl ConformanceTraversal<'_> {
    fn push_violation(
        &mut self,
        interface: &Symbol,
        parent_of: Option<&Symbol>,
        method: InterfaceMethodViolation,
    ) {
        let interface_name = unqualified_name(interface);
        let parent_of = parent_of.map(|interface| unqualified_name(interface.as_str()));
        if let Some(group) = self.violations.iter_mut().find(|group| {
            group.interface_name == interface_name && group.parent_of.as_deref() == parent_of
        }) {
            group.methods.push(method);
            return;
        }
        self.violations.push(InterfaceViolation {
            interface_name: interface_name.to_string(),
            parent_of: parent_of.map(String::from),
            methods: vec![method],
        });
    }
}

struct ConformanceSite<'a> {
    ty: &'a Type,
    symbol_methods: &'a Methods,
    interface_qualified_id: &'a str,
    interface_is_public: bool,
    receiver_id: Option<&'a str>,
}

impl InferCtx<'_> {
    fn method_in_value_method_set(&self, receiver: &Type, method: &str) -> bool {
        let store = self.store;
        let resolved = store.deep_resolve_alias(&receiver.strip_refs().resolve_in(&self.env));
        let Some(id) = resolved.get_qualified_id() else {
            return false;
        };
        let owner = if store.get_method(id, method).is_some() {
            id.to_string()
        } else {
            match crate::checker::promotion::resolve_selector(store, &resolved, method) {
                crate::checker::promotion::Resolution::Found(member) => {
                    member.declaring_type.to_string()
                }
                _ => return false,
            }
        };
        if !owner.starts_with(GO_IMPORT_PREFIX) {
            return false;
        }
        store.get_method(&owner, method).is_some_and(|method| {
            method
                .go_hints
                .iter()
                .any(|hint| hint == "value_method_set")
        })
    }

    pub(crate) fn check_concrete_bound(&mut self, ty: &Type, bound: &Type, span: &Span) {
        let bound = self.store.deep_resolve_alias(bound);
        if !self.store.is_interface(&bound) {
            return;
        }
        if self.satisfies_interface(ty, &bound, span).is_ok() {
            let _ = self.check_pointer_receivers(ty, &bound, span);
        }
    }

    pub(super) fn satisfies_interface(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let interface_ty = self.store.deep_resolve_alias(interface_ty);
        let Type::Nominal {
            id: interface_qualified_id,
            ..
        } = &interface_ty
        else {
            return Err(vec![]);
        };
        let requirements =
            interface_requirements(&interface_ty, |id| self.store.get_definition(id));
        let requires_methods = !requirements.is_empty();
        let resolved = ty.resolve_in(&self.env);
        let (core, behind_ref) = self.store.peel_refs_and_aliases(&resolved);
        if behind_ref && self.store.is_interface(&core) && requires_methods {
            self.sink
                .push(diagnostics::infer::ref_to_interface_does_not_implement(
                    unqualified_name(interface_qualified_id),
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

        let Some(violations) = self.with_satisfaction_check(pair, |this| {
            let adapter_capable = this.adapter_capable_receiver(ty, interface_qualified_id);
            let mut check = ConformanceTraversal {
                receiver: ty,
                span: *span,
                adapter_capable,
                violations: Vec::new(),
            };
            this.collect_interface_violations(&mut check, &requirements);
            check.violations
        }) else {
            return Ok(());
        };

        let builtin_receiver = self.is_builtin_receiver(ty);

        if violations.is_empty() && !(builtin_receiver && requires_methods) {
            return Ok(());
        }

        let resolved = ty.resolve_in(&self.env);
        if let Some(sealed) = violations.iter().find(|v| {
            v.methods
                .iter()
                .filter_map(|method| match method {
                    InterfaceMethodViolation::Missing(method) => Some(method),
                    InterfaceMethodViolation::Incompatible { .. } => None,
                })
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
                    unqualified_name(interface_qualified_id),
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
                    unqualified_name(interface_qualified_id),
                    &type_name,
                    *span,
                ));
        } else {
            let type_name = resolved
                .get_name()
                .map_or_else(|| resolved.to_string(), str::to_owned);
            self.sink
                .push(diagnostics::infer::interface_not_implemented(
                    unqualified_name(interface_qualified_id),
                    &type_name,
                    &violations,
                    *span,
                ));
        }
        Err(violations)
    }

    fn with_satisfaction_check<T>(
        &mut self,
        pair: (String, String),
        check: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        if !self.satisfying_stack.insert(pair.clone()) {
            return None;
        }
        let result = check(self);
        let removed = self.satisfying_stack.remove(&pair);
        debug_assert!(removed, "satisfaction check must remove its own guard");
        Some(result)
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
        interface_ty: &Type,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let store = self.store;
        if store.peel_alias(ty).is_ref() {
            return Ok(());
        }
        let interface_ty = store.deep_resolve_alias(interface_ty);
        let Some(interface_qualified_id) = interface_ty.get_qualified_id() else {
            return Ok(());
        };
        let methods = self.get_all_methods(store, ty);
        let ptr_methods = interface_requirements(&interface_ty, |id| store.get_definition(id))
            .into_iter()
            .filter_map(|requirement| {
                let interface_is_public = store
                    .get_definition(&requirement.declaring_interface)
                    .is_some_and(|d| d.visibility.is_public());
                let (impl_name, method_ty) = syntax::go_names::conformance_method(
                    &methods,
                    requirement.declaring_interface.as_str(),
                    interface_is_public,
                    requirement.name.as_str(),
                    &|candidate| self.conformance_candidate(ty, candidate),
                )?;
                let func = match method_ty {
                    Type::Forall { body, .. } => body.as_ref(),
                    other => other,
                };
                let has_pointer_receiver = matches!(func, Type::Function(f)
                    if f.params.first().is_some_and(|param| param.ty.is_ref()));
                (has_pointer_receiver && !self.method_in_value_method_set(ty, impl_name))
                    .then(|| impl_name.to_string())
            })
            .collect::<Vec<_>>();

        if ptr_methods.is_empty() {
            return Ok(());
        }

        let type_name = ty.get_name().map_or_else(|| ty.to_string(), str::to_owned);
        self.sink
            .push(diagnostics::infer::pointer_receiver_interface_mismatch(
                unqualified_name(interface_qualified_id),
                &type_name,
                &ptr_methods,
                *span,
            ));
        Err(vec![])
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
        if self.store.get_interface(id).is_some() {
            self.store
                .get_all_methods(resolved, &Default::default())
                .get(method)?;
        } else {
            self.store.get_method(id, method)?;
        }
        // UFCS-lowered methods emit as free functions, not selectors.
        Some(syntax::go_names::ConformanceCandidate::Resolved {
            depth: 0,
            owner: id.into(),
            shadowed: self.store.is_ufcs_method(id, method),
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
        let crate::checker::promotion::MemberKind::Method(promoted) = &member.kind else {
            return None;
        };
        let selector = if promoted.visibility.is_public() {
            syntax::go_names::snake_to_camel(method)
        } else {
            syntax::go_names::unexported_method_go_name(method)
        };
        let shadowed = promotion::field_selector_depth(store, resolved, &selector)
            .is_some_and(|field_depth| field_depth <= member.depth);
        Some(syntax::go_names::ConformanceCandidate::Resolved {
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
            let interface_ty = self.store.deep_resolve_alias(bound);
            interface_ty.get_qualified_id()?;
            self.store
                .get_all_methods(&interface_ty, &Default::default())
                .get(method)?;
            Some(syntax::go_names::ConformanceCandidate::Resolved {
                depth: 0,
                owner: qualified.as_eco().clone(),
                shadowed: false,
            })
        })
    }

    /// Mirror of emit's `needs_adapter` precondition.
    fn adapter_capable_receiver(&self, ty: &Type, interface_qualified_id: &str) -> bool {
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
        self.own_methods_cover_interface(own, id, interface_qualified_id)
    }

    fn own_methods_cover_interface(
        &self,
        own: &Methods,
        own_id: &str,
        interface_qualified_id: &str,
    ) -> bool {
        let store = self.store;
        let own_candidate = |name: &str| {
            store
                .get_method(own_id, name)
                .map(|_| syntax::go_names::ConformanceCandidate::Resolved {
                    depth: 0,
                    owner: own_id.into(),
                    shadowed: self.store.is_ufcs_method(own_id, name),
                })
                .unwrap_or(syntax::go_names::ConformanceCandidate::Unresolved)
        };
        let interface_ty = Type::Nominal {
            id: interface_qualified_id.into(),
            params: vec![],
        };
        interface_requirements(&interface_ty, |id| store.get_definition(id))
            .into_iter()
            .all(|requirement| {
                let interface_is_public = store
                    .get_definition(&requirement.declaring_interface)
                    .is_some_and(|d| d.visibility.is_public());
                syntax::go_names::conformance_method(
                    own,
                    requirement.declaring_interface.as_str(),
                    interface_is_public,
                    requirement.name.as_str(),
                    &own_candidate,
                )
                .is_some()
            })
    }

    fn collect_interface_violations(
        &mut self,
        check: &mut ConformanceTraversal<'_>,
        requirements: &[InterfaceRequirement],
    ) {
        let store = self.store;
        let ty = check.receiver;
        let span = check.span;
        let symbol_methods = self.get_all_methods(store, ty);
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

        for requirement in requirements {
            let interface_qualified_id = &requirement.declaring_interface;
            let method_name = &requirement.name;
            let method_ty = &requirement.ty;
            let interface_is_public = store
                .get_definition(interface_qualified_id)
                .is_some_and(|d| d.visibility.is_public());
            let site = ConformanceSite {
                ty,
                symbol_methods: &symbol_methods,
                interface_qualified_id,
                interface_is_public,
                receiver_id: receiver_id.as_ref().map(|id| id.as_str()),
            };
            let selected = self.select_impl_method(&site, method_name);
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
                                unqualified_name(interface_qualified_id),
                                method_name,
                                &receiver_generics,
                                span,
                            ),
                        );
                    }
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Missing(MissingMethod {
                            name: method_name.to_string(),
                            signature: method_ty.clone(),
                            private_candidate: None,
                        }),
                    );
                    continue;
                }
                SelectedMethod::Missing => {
                    let private_candidate = self.private_method_hint(&site, method_name, method_ty);
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Missing(MissingMethod {
                            name: method_name.to_string(),
                            signature: method_ty.clone(),
                            private_candidate,
                        }),
                    );
                    continue;
                }
            };

            let signature =
                self.check_method_signature(&site, method_name, method_ty, &symbol_method);

            match signature {
                SignatureCheck::Matched => {
                    self.validate_comma_ok_abi(
                        check,
                        interface_qualified_id,
                        method_name,
                        requirement
                            .method
                            .go_hints
                            .iter()
                            .any(|hint| hint == "comma_ok"),
                        impl_method_name.as_str(),
                    );
                    let spelling_pinned = syntax::go_names::interface_matches_by_source_name(
                        interface_qualified_id,
                        interface_is_public,
                    );
                    self.record_conformance_use(ty, impl_method_name.as_str(), spelling_pinned);
                }
                SignatureCheck::ReceiverMismatch => {
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Missing(MissingMethod {
                            name: method_name.to_string(),
                            signature: method_ty.clone(),
                            private_candidate: None,
                        }),
                    );
                }
                SignatureCheck::Incompatible { expected, actual } => {
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Incompatible {
                            name: method_name.to_string(),
                            expected,
                            actual,
                        },
                    );
                }
            }
        }
    }

    fn select_impl_method(&self, site: &ConformanceSite<'_>, method_name: &str) -> SelectedMethod {
        let selected = syntax::go_names::conformance_method(
            site.symbol_methods,
            site.interface_qualified_id,
            site.interface_is_public,
            method_name,
            &|name| self.conformance_candidate(site.ty, name),
        );
        let ufcs_probe = match &selected {
            Some((name, _)) => name.as_str(),
            None => method_name,
        };
        if site
            .receiver_id
            .is_some_and(|id| self.store.is_ufcs_method(id, ufcs_probe))
        {
            return SelectedMethod::UfcsOnly;
        }
        match selected {
            Some((name, method)) => SelectedMethod::Found(name.clone(), method.clone()),
            None => SelectedMethod::Missing,
        }
    }

    fn private_method_hint(
        &mut self,
        site: &ConformanceSite<'_>,
        method_name: &str,
        method_ty: &Type,
    ) -> Option<String> {
        let (impl_name, impl_method) = syntax::go_names::conformance_method_if_public(
            site.symbol_methods,
            site.interface_qualified_id,
            site.interface_is_public,
            method_name,
            &|name| self.conformance_candidate(site.ty, name),
        )?;
        let impl_name = impl_name.to_string();
        let impl_method = impl_method.clone();
        let signature = self.check_method_signature(site, method_name, method_ty, &impl_method);
        matches!(signature, SignatureCheck::Matched).then_some(impl_name)
    }

    fn check_method_signature(
        &mut self,
        site: &ConformanceSite<'_>,
        method_name: &str,
        method_ty: &Type,
        symbol_method: &Type,
    ) -> SignatureCheck {
        let store = self.store;

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
            site.interface_qualified_id,
            method_name,
            method_ty,
            &impl_method_without_receiver,
            store,
        )
        .unwrap_or_else(|| impl_method_without_receiver.clone());

        enum Mismatch {
            Receiver,
            Signature,
        }

        let candidate_ty = site.ty.strip_refs().resolve_in(&self.env);
        let mut resolved_impl_method = None;
        let sig_match = self.in_invariant_position(|ctx| {
            ctx.speculatively(|this| {
                if let Some(receiver) = &receiver_to_pin {
                    this.try_unify(receiver, &candidate_ty, &Span::dummy())
                        .map_err(|_| Mismatch::Receiver)?;
                }
                this.try_unify(
                    &strip_bounds(method_ty),
                    &strip_bounds(&impl_for_unify),
                    &Span::dummy(),
                )
                .map_err(|_| {
                    resolved_impl_method = Some(impl_method_without_receiver.resolve_in(&this.env));
                    Mismatch::Signature
                })
            })
        });

        match sig_match {
            Ok(()) => SignatureCheck::Matched,
            Err(Mismatch::Receiver) => SignatureCheck::ReceiverMismatch,
            Err(Mismatch::Signature) => SignatureCheck::Incompatible {
                expected: method_ty.clone(),
                actual: resolved_impl_method.unwrap_or(impl_method_without_receiver),
            },
        }
    }

    fn validate_comma_ok_abi(
        &mut self,
        check: &ConformanceTraversal<'_>,
        interface_qualified_id: &str,
        method_name: &str,
        interface_comma_ok: bool,
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
        let crate::checker::promotion::MemberKind::Method(method) = member.kind else {
            return;
        };
        let selected_comma_ok = method.go_hints.iter().any(|hint| hint == "comma_ok");
        let adapter_reconciles = check.adapter_capable && interface_comma_ok && !selected_comma_ok;
        if interface_comma_ok != selected_comma_ok && !adapter_reconciles {
            self.sink.push(diagnostics::embed::comma_ok_abi_mismatch(
                unqualified_name(interface_qualified_id),
                method_name,
                check.span,
            ));
        }
    }

    fn record_conformance_use(&mut self, ty: &Type, impl_method_name: &str, spelling_pinned: bool) {
        let store = self.store;
        if let Type::Nominal { id, .. } = ty.strip_refs().resolve_in(&self.env)
            && let Some(package) = store.package_for_qualified_name(id.as_str())
            && let Some(type_name) = id.as_str().get(package.len() + 1..)
            && !type_name.contains('.')
        {
            self.facts.mark_method_used_for_interface(
                package.to_string(),
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

enum SignatureCheck {
    Matched,
    ReceiverMismatch,
    Incompatible { expected: Type, actual: Type },
}

pub(crate) fn interface_requires_methods(store: &Store, id: &str) -> bool {
    let interface_ty = Type::Nominal {
        id: id.into(),
        params: vec![],
    };
    !interface_requirements(&interface_ty, |id| store.get_definition(id)).is_empty()
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

    let hints = store
        .get_method(interface_qualified_id, method_name)
        .map(|method| method.go_hints.as_slice())
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
