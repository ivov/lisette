use crate::checker::EnvResolve;
use crate::checker::promotion;
use crate::checker::resolution::MethodSelection;
use crate::checker::sealing::is_unexported_key;
use crate::store::Store;
use diagnostics::infer::{InterfaceMethodViolation, InterfaceViolation, MissingMethod};
use syntax::ast::Span;
use syntax::program::CheckedMethod;
use syntax::program::ConformanceWitness;
use syntax::program::GoHint;
use syntax::program::GoInteropContract;
use syntax::program::{
    DefinitionBody, InterfaceRequirement, Methods, interface_declares_any_method,
    interface_requirements,
};
use syntax::types::{GO_IMPORT_PREFIX, Symbol, Type, unqualified_name};
use syntax::types::{method_receiver_substitution, substitute};

use crate::checker::infer::InferCtx;
use syntax::go_names;
use syntax::go_names::ConformanceCandidate;

struct ConformanceTraversal<'a> {
    receiver: &'a Type,
    span: Span,
    adapter_capable: bool,
    violations: Vec<InterfaceViolation>,
    reported_violations: usize,
    methods: Vec<CheckedMethod>,
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
    fn conformance_receiver(&self, ty: &Type) -> Type {
        promotion::selector_receiver(&ty.resolve_in(&self.env), |id| {
            self.store.get_definition(id)
        })
    }

    fn method_in_value_method_set(&self, receiver: &Type, method: &str) -> bool {
        let store = self.store;
        let resolved = self.conformance_receiver(receiver);
        let Some(id) = resolved.get_qualified_id() else {
            return false;
        };
        let owner = if store.get_method(id, method).is_some() {
            id.to_string()
        } else {
            match promotion::resolve_selector(store, &resolved, method) {
                promotion::Resolution::Found(member) => member.declaring_type.to_string(),
                _ => return false,
            }
        };
        if !owner.starts_with(GO_IMPORT_PREFIX) {
            return false;
        }
        store
            .get_method(&owner, method)
            .is_some_and(|method| method.go_hints.contains(&GoHint::ValueMethodSet))
    }

    pub(crate) fn check_concrete_bound(&mut self, ty: &Type, bound: &Type, span: &Span) {
        self.check_concrete_bound_with_hint(ty, bound, span, None);
    }

    pub(super) fn check_concrete_bound_with_hint(
        &mut self,
        ty: &Type,
        bound: &Type,
        span: &Span,
        parameter_hint: Option<diagnostics::infer::BoundParameterHint<'_>>,
    ) {
        let bound = self.store.peel_alias(bound);
        if !self.store.is_interface(&bound) {
            return;
        }
        let store = self.store;
        let resolved = self.peel_resolved(ty);
        if resolved.contains_error() || bound.contains_error() {
            return;
        }
        let methods = if store.points_to_ref(&resolved) {
            Methods::default()
        } else {
            self.get_all_methods(store, &resolved)
        };
        if self
            .satisfies_interface_with_methods(&resolved, &bound, span, &methods)
            .is_ok()
            && self
                .check_pointer_receivers(&resolved, &bound, span, parameter_hint)
                .is_ok()
        {
            self.check_bound_method_shapes(&resolved, &bound, span);
        }
    }

    /// A type argument gets no adapter, so its methods need the Go shapes the bound requires.
    fn check_bound_method_shapes(&mut self, ty: &Type, bound: &Type, span: &Span) {
        let store = self.store;
        let receiver = self.conformance_receiver(ty);
        let Some(receiver_id) = receiver.get_qualified_id() else {
            return;
        };
        let receiver_is_go = receiver_id.starts_with(GO_IMPORT_PREFIX);
        let is_parameter = |ty: &Type| matches!(store.peel_alias(ty), Type::Parameter(_));
        for requirement in interface_requirements(bound, |id| store.get_definition(id)) {
            let declared_by_go = requirement
                .declaring_interface
                .starts_with(GO_IMPORT_PREFIX);
            let unsupported = if receiver_is_go {
                !declared_by_go
                    && requirement
                        .method
                        .ty
                        .unwrap_forall()
                        .get_function_ret()
                        .is_some_and(is_parameter)
            } else {
                declared_by_go && self.lacks_go_method_shape(&receiver, &requirement)
            };
            if unsupported {
                let type_name = receiver
                    .display_name()
                    .map_or_else(|| receiver.to_string(), |name| name.to_string());
                self.sink
                    .push(diagnostics::infer::unsupported_bound_argument(
                        unqualified_name(&requirement.declaring_interface),
                        &type_name,
                        *span,
                    ));
                return;
            }
        }
    }

    /// A slot declared as a bare type parameter never has the Go shape of a lowered type.
    fn lacks_go_method_shape(&self, receiver: &Type, requirement: &InterfaceRequirement) -> bool {
        let store = self.store;
        let promotion::Resolution::Found(member) =
            promotion::resolve_selector(store, receiver, &requirement.name)
        else {
            return false;
        };
        let Some(method) = store.get_method(&member.declaring_type, &requirement.name) else {
            return false;
        };
        let (Some(declared), Some(go)) = (
            method.ty.unwrap_forall().as_function_type(),
            requirement.ty.as_function_type(),
        ) else {
            return false;
        };
        let is_parameter = |ty: &Type| matches!(store.peel_alias(ty), Type::Parameter(_));
        let go_return = store.peel_alias(&go.return_type);
        let return_differs = is_parameter(&declared.return_type)
            && (go_return.is_result()
                || go_return.is_option()
                || go_return.is_partial()
                || go_return.is_tuple());
        return_differs
            || declared
                .params
                .iter()
                .skip(1)
                .zip(&go.params)
                .any(|(declared, go)| {
                    is_parameter(&declared.ty) && store.peel_alias(&go.ty).is_option()
                })
    }

    pub(super) fn satisfies_interface(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let store = self.store;
        let methods = self.get_all_methods(store, ty);
        self.satisfies_interface_with_methods(ty, interface_ty, span, &methods)
    }

    pub(super) fn conforms_to_interface(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
        span: &Span,
    ) -> Result<(), Vec<InterfaceViolation>> {
        self.satisfies_interface(ty, interface_ty, span)
            .and_then(|()| self.check_pointer_receivers(ty, interface_ty, span, None))
    }

    fn satisfies_interface_with_methods(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
        span: &Span,
        methods: &Methods,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let interface_ty = self.store.peel_alias(interface_ty);
        let Type::Nominal {
            id: interface_qualified_id,
            ..
        } = &interface_ty
        else {
            return Err(vec![]);
        };
        let requirements =
            interface_requirements(&interface_ty, |id| self.store.get_definition(id));
        if self.has_conflicting_requirements(&requirements) {
            return Err(vec![]);
        }
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

        let owner = |ty: &Type| match ty {
            Type::Nominal { id, .. } => Type::Nominal {
                id: id.clone(),
                params: Vec::new(),
                writable: false,
            },
            other => other.clone(),
        };
        let pair = (owner(&resolved), owner(&interface_ty));

        let Some((violations, reported_violations, checked_methods)) = self
            .with_satisfaction_check(pair, |this| {
                let adapter_capable = this.adapter_capable_receiver(ty);
                let mut check = ConformanceTraversal {
                    receiver: ty,
                    span: *span,
                    adapter_capable,
                    violations: Vec::new(),
                    reported_violations: 0,
                    methods: Vec::new(),
                };
                this.collect_interface_violations(&mut check, &requirements, methods);
                (check.violations, check.reported_violations, check.methods)
            })
        else {
            return Ok(());
        };

        let builtin_receiver = self.is_builtin_receiver(ty);

        if violations.is_empty() && !(builtin_receiver && requires_methods) {
            if checked_methods.len() == requirements.len() {
                let witness = ConformanceWitness {
                    receiver: ty.resolve_in(&self.env),
                    interface: interface_ty.clone(),
                    methods: checked_methods,
                };
                self.facts
                    .value_uses
                    .selected_witnesses
                    .push(witness.clone());
                // A type parameter names no one type, so its conformance holds only in this scope.
                if !matches!(self.conformance_receiver(ty), Type::Parameter(_)) {
                    let store = self.store;
                    self.facts
                        .conformances
                        .insert(witness, |id| store.get_definition(id));
                }
            }
            return Ok(());
        }

        let violation_count: usize = violations.iter().map(|v| v.methods.len()).sum();
        if violation_count > 0 && violation_count == reported_violations {
            return Err(violations);
        }

        let resolved = ty.resolve_in(&self.env);
        if let Some(sealed) = violations.iter().find(|v| {
            v.methods
                .iter()
                .filter_map(|method| match method {
                    InterfaceMethodViolation::Missing(method) => Some(method),
                    InterfaceMethodViolation::Incompatible { .. } => None,
                })
                .any(|method| is_unexported_key(&method.name))
        }) {
            let type_name = resolved
                .display_name()
                .map_or_else(|| resolved.to_string(), |name| name.to_string());
            self.sink
                .push(diagnostics::infer::sealed_interface_not_satisfiable(
                    &sealed.interface_name,
                    &type_name,
                    resolved
                        .get_qualified_id()
                        .is_some_and(|id| id.starts_with("go:")),
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
        let type_name = resolved
            .display_name()
            .map_or_else(|| resolved.to_string(), |name| name.to_string());
        let diagnostic = if let Some(wrapper) = wrapper {
            diagnostics::infer::wrapper_does_not_implement_interface(
                unqualified_name(interface_qualified_id),
                wrapper,
                &resolved,
                *span,
            )
        } else if builtin_receiver {
            diagnostics::infer::builtin_type_cannot_implement_interface(
                unqualified_name(interface_qualified_id),
                &type_name,
                *span,
            )
        } else if blocked_by_type_parameter_permission(&violations) {
            diagnostics::infer::unsupported_generic_go_interface(
                unqualified_name(interface_qualified_id),
                &type_name,
                *span,
            )
        } else {
            let foreign_package = resolved
                .get_qualified_id()
                .filter(|id| id.starts_with("go:"))
                .and_then(|id| id.rsplit_once('.').map(|(package, _)| package));
            diagnostics::infer::interface_not_implemented(
                unqualified_name(interface_qualified_id),
                &type_name,
                &violations,
                foreign_package,
                *span,
            )
        };
        self.sink.push(diagnostic);
        Err(violations)
    }

    fn has_conflicting_requirements(&self, requirements: &[InterfaceRequirement]) -> bool {
        requirements.iter().enumerate().any(|(i, requirement)| {
            requirements[..i]
                .iter()
                .filter(|earlier| earlier.name == requirement.name)
                .any(|earlier| {
                    self.store.normalized_annotation_type(&earlier.ty)
                        != self.store.normalized_annotation_type(&requirement.ty)
                })
        })
    }

    fn with_satisfaction_check<T>(
        &mut self,
        pair: (Type, Type),
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
        let resolved = self.conformance_receiver(ty);
        match resolved.strip_refs() {
            Type::Simple(_) | Type::Compound { .. } | Type::Array { .. } => true,
            Type::Nominal { id, .. } => {
                id.as_str().starts_with("prelude.")
                    && self.store.get_interface(id.as_str()).is_none()
            }
            _ => false,
        }
    }

    pub(super) fn check_pointer_receivers(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
        span: &Span,
        parameter_hint: Option<diagnostics::infer::BoundParameterHint<'_>>,
    ) -> Result<(), Vec<InterfaceViolation>> {
        let store = self.store;
        let peeled = store.peel_alias(ty);
        let value_is_ref = peeled.is_ref();
        if value_is_ref && peeled.is_writable() {
            return Ok(());
        }
        let interface_ty = store.peel_alias(interface_ty);
        let Some(interface_qualified_id) = interface_ty.get_qualified_id() else {
            return Ok(());
        };
        let ptr_methods = self
            .reference_receiver_conformance(ty, &interface_ty)
            .into_iter()
            .filter(|(_, writes_through_receiver)| !value_is_ref || *writes_through_receiver)
            .collect::<Vec<_>>();

        if ptr_methods.is_empty() {
            return Ok(());
        }

        let type_name = ty
            .display_name()
            .map_or_else(|| ty.to_string(), |name| name.to_string());
        let diagnostic = if value_is_ref {
            let pointee = self.conformance_receiver(ty);
            let pointee_name = pointee
                .display_name()
                .map_or_else(|| pointee.to_string(), |name| name.to_string());
            let names = ptr_methods
                .into_iter()
                .map(|(impl_name, _)| impl_name)
                .collect::<Vec<_>>();
            diagnostics::infer::interface_needs_writable_receiver(
                unqualified_name(interface_qualified_id),
                &pointee_name,
                &names,
                *span,
            )
        } else {
            diagnostics::infer::pointer_receiver_interface_mismatch(
                unqualified_name(interface_qualified_id),
                &type_name,
                &ptr_methods,
                *span,
                parameter_hint,
            )
        };
        self.sink.push(diagnostic);
        Err(vec![])
    }

    /// The conformance methods of `interface_ty` on `ty` that take a reference
    /// receiver, each paired with whether that receiver is writable. Methods in
    /// the value method set are absent, because a value satisfies those.
    fn reference_receiver_conformance(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
    ) -> Vec<(String, bool)> {
        let store = self.store;
        let receiver = ty.resolve_in(&self.env);
        if let Some(witness) = self
            .facts
            .conformances
            .get(&receiver, interface_ty, |id| store.get_definition(id))
        {
            return witness
                .methods
                .iter()
                .filter_map(|method| {
                    if !method.pointer_receiver
                        || self.method_in_value_method_set(ty, &method.selected_name)
                    {
                        return None;
                    }
                    let receiver = &method
                        .signature
                        .unwrap_forall()
                        .get_function_params()?
                        .first()?
                        .ty;
                    Some((method.selected_name.to_string(), receiver.is_writable()))
                })
                .collect();
        }
        let methods = self.get_all_methods(store, ty);
        interface_requirements(interface_ty, |id| store.get_definition(id))
            .into_iter()
            .filter_map(|requirement| {
                let interface_is_public = store
                    .get_definition(&requirement.declaring_interface)
                    .is_some_and(|d| d.visibility.is_public());
                let (impl_name, method_ty) = go_names::conformance_method(
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
                let Type::Function(f) = func else {
                    return None;
                };
                let receiver = &f.params.first()?.ty;
                if !receiver.is_ref() || self.method_in_value_method_set(ty, impl_name) {
                    return None;
                }
                Some((impl_name.to_string(), receiver.is_writable()))
            })
            .collect()
    }

    /// A reference passed as `interface_ty` reaches a method that writes through
    /// the receiver, so the call can mutate the referent. The interface hides
    /// the receiver qualifier, so a caller cannot read this off the signature.
    pub(super) fn interface_writes_through_receiver(
        &mut self,
        ty: &Type,
        interface_ty: &Type,
    ) -> bool {
        let store = self.store;
        if !store.is_reference(ty) {
            return false;
        }
        let interface_ty = store.peel_alias(interface_ty);
        if interface_ty.get_qualified_id().is_none() {
            return false;
        }
        self.reference_receiver_conformance(ty, &interface_ty)
            .into_iter()
            .any(|(_, writes_through_receiver)| writes_through_receiver)
    }

    fn conformance_candidate(&self, receiver: &Type, method: &str) -> ConformanceCandidate {
        let resolved = self.conformance_receiver(receiver);
        self.own_candidate(&resolved, method)
            .or_else(|| self.promoted_candidate(&resolved, method))
            .or_else(|| self.bound_candidate(&resolved, method))
            .unwrap_or(ConformanceCandidate::Unresolved)
    }

    fn own_candidate(&self, resolved: &Type, method: &str) -> Option<ConformanceCandidate> {
        let id = resolved.get_qualified_id()?;
        if self.store.get_interface(id).is_some() {
            self.store.get_method_for_type(resolved, method)?;
        } else {
            self.store.get_method(id, method)?;
        }
        // UFCS-lowered methods emit as free functions, not selectors.
        Some(ConformanceCandidate::Resolved {
            depth: 0,
            owner: id.into(),
            shadowed: self.store.is_ufcs_method(id, method),
        })
    }

    fn promoted_candidate(&self, resolved: &Type, method: &str) -> Option<ConformanceCandidate> {
        let store = self.store;
        resolved.get_qualified_id()?;
        let promotion::Resolution::Found(member) =
            promotion::resolve_selector(store, resolved, method)
        else {
            return None;
        };
        let promotion::MemberKind::Method(promoted) = &member.kind else {
            return None;
        };
        let selector = if promoted.visibility.is_public() {
            go_names::snake_to_camel(method)
        } else {
            go_names::unexported_method_go_name(method)
        };
        let shadowed = promotion::field_selector_depth(store, resolved, &selector)
            .is_some_and(|field_depth| field_depth <= member.depth);
        Some(ConformanceCandidate::Resolved {
            depth: member.depth,
            owner: member.declaring_type.as_eco().clone(),
            shadowed,
        })
    }

    // Bound method sets merge as one Go constraint interface.
    fn bound_candidate(&self, resolved: &Type, method: &str) -> Option<ConformanceCandidate> {
        let Type::Parameter(name) = resolved else {
            return None;
        };
        self.method_of_type(self.store, resolved, method)?;
        Some(ConformanceCandidate::Resolved {
            depth: 0,
            owner: self.qualify_name(name).as_eco().clone(),
            shadowed: false,
        })
    }

    fn adapter_capable_receiver(&self, ty: &Type) -> bool {
        let resolved = self.conformance_receiver(ty);
        let Some(id) = resolved.get_qualified_id() else {
            return false;
        };
        !id.starts_with(GO_IMPORT_PREFIX)
            && matches!(
                self.store
                    .get_definition(id)
                    .map(|definition| &definition.body),
                Some(DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. })
            )
    }

    fn collect_interface_violations(
        &mut self,
        check: &mut ConformanceTraversal<'_>,
        requirements: &[InterfaceRequirement],
        symbol_methods: &Methods,
    ) {
        let store = self.store;
        let ty = check.receiver;
        let span = check.span;
        let resolved_receiver = self.conformance_receiver(ty);
        let receiver_requirements =
            interface_requirements(&resolved_receiver, |id| self.store.get_definition(id));
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
                symbol_methods,
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
                            .display_name()
                            .map_or_else(|| resolved_receiver.to_string(), |name| name.to_string());
                        self.sink.push(
                            diagnostics::infer::specialized_impl_cannot_satisfy_interface(
                                &type_name,
                                unqualified_name(interface_qualified_id),
                                method_name,
                                &receiver_generics,
                                span,
                            ),
                        );
                        check.reported_violations += 1;
                    }
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Missing(MissingMethod {
                            name: method_name.to_string(),
                            signature: method_ty.clone(),
                            private_candidate: None,
                            private_to_package: None,
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
                            private_to_package: None,
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
                        requirement.method.go_hints.contains(&GoHint::CommaOk),
                        impl_method_name.as_str(),
                    );
                    let spelling_pinned = go_names::interface_matches_by_source_name(
                        interface_qualified_id,
                        interface_is_public,
                    );
                    self.record_conformance_use(ty, impl_method_name.as_str(), spelling_pinned);
                    let selected =
                        promotion::resolve_selector(store, &resolved_receiver, &impl_method_name);
                    let promotion::Resolution::Found(member) = selected else {
                        if let Type::Parameter(_) = resolved_receiver
                            && let Some(MethodSelection {
                                bound: Some(bound),
                                method,
                                ..
                            }) =
                                self.method_of_type(store, &resolved_receiver, &impl_method_name)
                            && let Some(bound_id) = bound.get_qualified_id()
                        {
                            let selected_requirement =
                                bound_requirement(store, &bound, &impl_method_name);
                            check.methods.push(CheckedMethod {
                                required_interface: interface_qualified_id.clone(),
                                required_name: method_name.clone(),
                                selected_definition: Symbol::from(bound_id)
                                    .with_segment(&impl_method_name),
                                selected_name: impl_method_name.clone(),
                                selected_requirement,
                                pointer_receiver: symbol_method
                                    .unwrap_forall()
                                    .get_function_params()
                                    .and_then(|parameters| parameters.first())
                                    .is_some_and(|receiver| receiver.ty.is_ref()),
                                signature: symbol_method.clone(),
                                declared_signature: symbol_method.clone(),
                                instantiated_declared_signature: symbol_method
                                    .unwrap_forall()
                                    .clone(),
                                is_exported: method.visibility.is_public(),
                            });
                        } else if !matches!(resolved_receiver, Type::Parameter(_)) {
                            check.push_violation(
                                interface_qualified_id,
                                requirement.parent_of.as_ref(),
                                InterfaceMethodViolation::Missing(MissingMethod {
                                    name: method_name.to_string(),
                                    signature: method_ty.clone(),
                                    private_candidate: None,
                                    private_to_package: None,
                                }),
                            );
                        }
                        continue;
                    };
                    let promotion::MemberKind::Method(selected_method) = &member.kind else {
                        continue;
                    };
                    let owner = member.declaring_type;
                    let is_exported = selected_method.visibility.is_public();
                    if !is_exported
                        && let Some(hidden) = self.private_method_hidden_from_conversion(
                            &owner,
                            &impl_method_name,
                            interface_qualified_id,
                            &span,
                        )
                    {
                        check.push_violation(
                            interface_qualified_id,
                            requirement.parent_of.as_ref(),
                            InterfaceMethodViolation::Missing(MissingMethod {
                                name: method_name.to_string(),
                                signature: method_ty.clone(),
                                private_candidate: Some(impl_method_name.to_string()),
                                private_to_package: hidden.private_to_package,
                            }),
                        );
                        continue;
                    }
                    let declaring_instance = *member.declaring_instance;
                    let declared_signature = store
                        .get_method(&owner, &impl_method_name)
                        .map(|method| method.ty.clone())
                        .unwrap_or_else(|| symbol_method.clone());
                    let substitutions = declared_signature
                        .unwrap_forall()
                        .get_function_params()
                        .and_then(|parameters| parameters.first())
                        .and_then(|receiver| {
                            method_receiver_substitution(&receiver.ty, &declaring_instance)
                        })
                        .unwrap_or_default();
                    let instantiated_declared_signature =
                        substitute(declared_signature.unwrap_forall(), &substitutions);
                    let selected_requirement = receiver_requirements
                        .iter()
                        .find(|candidate| candidate.name == impl_method_name)
                        .cloned();
                    check.methods.push(CheckedMethod {
                        required_interface: interface_qualified_id.clone(),
                        required_name: method_name.clone(),
                        selected_definition: owner.with_segment(&impl_method_name),
                        selected_name: impl_method_name.clone(),
                        selected_requirement,
                        pointer_receiver: symbol_method
                            .unwrap_forall()
                            .get_function_params()
                            .and_then(|parameters| parameters.first())
                            .is_some_and(|receiver| receiver.ty.is_ref()),
                        signature: symbol_method.clone(),
                        declared_signature,
                        instantiated_declared_signature,
                        is_exported,
                    });
                }
                SignatureCheck::ReceiverMismatch => {
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Missing(MissingMethod {
                            name: method_name.to_string(),
                            signature: method_ty.clone(),
                            private_candidate: None,
                            private_to_package: None,
                        }),
                    );
                }
                SignatureCheck::Incompatible { expected, actual } => {
                    let impl_span = site
                        .symbol_methods
                        .get(impl_method_name.as_str())
                        .and_then(|method| method.name_span);
                    let through_type_parameter = interface_qualified_id
                        .starts_with(GO_IMPORT_PREFIX)
                        && permission_gap_only_at_type_parameters(
                            &requirement.method.ty,
                            &expected,
                            &actual,
                        );
                    check.push_violation(
                        interface_qualified_id,
                        requirement.parent_of.as_ref(),
                        InterfaceMethodViolation::Incompatible {
                            name: method_name.to_string(),
                            expected,
                            actual,
                            impl_span,
                            through_type_parameter,
                        },
                    );
                }
            }
        }
    }

    fn select_impl_method(&self, site: &ConformanceSite<'_>, method_name: &str) -> SelectedMethod {
        let selected = go_names::conformance_method(
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
        let (impl_name, impl_method) = go_names::conformance_method_if_public(
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

        let impl_for_unify = covariant_return_adjustment(
            site.interface_qualified_id,
            method_name,
            method_ty,
            &impl_method_without_receiver,
            store,
        )
        .unwrap_or_else(|| impl_method_without_receiver.clone());

        let go_interface = site.interface_qualified_id.starts_with(GO_IMPORT_PREFIX);

        enum Mismatch {
            Receiver,
            Signature,
        }

        let candidate_ty = self.conformance_receiver(site.ty);
        let mut resolved_impl_method = None;
        #[expect(
            clippy::map_err_ignore,
            reason = "a speculative unification needs only which part failed, not why"
        )]
        let sig_match = self.in_invariant_position(|context| {
            context.unify_speculatively(|this| {
                if let Some(receiver) = &receiver_to_pin {
                    this.try_unify(receiver, &candidate_ty, &Span::dummy())
                        .map_err(|_| Mismatch::Receiver)?;
                }
                let required = signature_to_match(method_ty, go_interface);
                let provided = signature_to_match(&impl_for_unify, go_interface);
                this.try_unify(&required, &provided, &Span::dummy())
                    .map_err(|_| {
                        resolved_impl_method = Some(this.resolve(&impl_method_without_receiver));
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
        let resolved_ty = self.conformance_receiver(check.receiver);
        if resolved_ty.get_qualified_id().is_none() {
            return;
        }
        let promotion::Resolution::Found(member) =
            promotion::resolve_selector(store, &resolved_ty, impl_method_name)
        else {
            return;
        };
        let promotion::MemberKind::Method(method) = member.kind else {
            return;
        };
        let selected_comma_ok = method.go_hints.contains(&GoHint::CommaOk);
        let adapter_reconciles = check.adapter_capable && interface_comma_ok && !selected_comma_ok;
        if interface_comma_ok != selected_comma_ok && !adapter_reconciles {
            self.sink.push(diagnostics::embed::comma_ok_abi_mismatch(
                unqualified_name(interface_qualified_id),
                method_name,
                check.span,
            ));
        }
    }

    fn private_method_hidden_from_conversion(
        &self,
        owner: &str,
        method_name: &str,
        interface_id: &str,
        span: &Span,
    ) -> Option<HiddenPrivateMethod> {
        let store = self.store;
        let method_package = store.package_for_qualified_name(owner)?;
        if method_package.starts_with(GO_IMPORT_PREFIX) || method_package == "prelude" {
            return None;
        }
        if store.package_for_qualified_name(interface_id) == Some(method_package) {
            return None;
        }
        let conversion_package = store
            .get_file(span.file_id)
            .map_or_else(|| self.cursor.package_id(), |file| file.package_id.as_str());
        if conversion_package != method_package {
            return Some(HiddenPrivateMethod {
                private_to_package: Some(method_package.to_string()),
            });
        }
        (!store.go_method_policy().exports_method(method_name)).then_some(HiddenPrivateMethod {
            private_to_package: None,
        })
    }

    fn record_conformance_use(&mut self, ty: &Type, impl_method_name: &str, spelling_pinned: bool) {
        let store = self.store;
        if let Type::Nominal { id, .. } = self.conformance_receiver(ty)
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

struct HiddenPrivateMethod {
    private_to_package: Option<String>,
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
        writable: false,
    };
    interface_declares_any_method(&interface_ty, |id| store.get_definition(id))
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

    let declared = store.get_method(interface_qualified_id, method_name)?;
    let contract = GoInteropContract::new(&declared.ty, interface_method, &declared.go_hints);
    if !contract.permits_nullable_lift(impl_ret, |id| store.get_definition(id)) {
        return None;
    }

    Some(impl_f.rebuild(
        impl_f.params.clone(),
        impl_f.bounds.clone(),
        iface_ret.clone(),
    ))
}

fn blocked_by_type_parameter_permission(violations: &[InterfaceViolation]) -> bool {
    let mut methods = violations
        .iter()
        .flat_map(|violation| &violation.methods)
        .peekable();
    methods.peek().is_some()
        && methods.all(|method| {
            matches!(
                method,
                InterfaceMethodViolation::Incompatible {
                    through_type_parameter: true,
                    ..
                }
            )
        })
}

fn permission_gap_only_at_type_parameters(declared: &Type, expected: &Type, actual: &Type) -> bool {
    let declared = declared.unwrap_forall();
    match (
        erase_at_parameters(declared, expected.unwrap_forall()),
        erase_at_parameters(declared, actual.unwrap_forall()),
    ) {
        (Some(expected), Some(actual)) => expected == actual,
        _ => false,
    }
}

fn erase_at_parameters(declared: &Type, ty: &Type) -> Option<Type> {
    match (declared, ty) {
        (Type::Parameter(_), _) => Some(without_function_bounds(&ty.without_permissions())),
        (Type::Function(declared), Type::Function(f)) => {
            if declared.params.len() != f.params.len() {
                return None;
            }
            let params = declared
                .params
                .iter()
                .zip(&f.params)
                .map(|(declared, param)| {
                    erase_at_parameters(&declared.ty, &param.ty).map(|ty| param.with_type(ty))
                })
                .collect::<Option<Vec<_>>>()?;
            let return_type = erase_at_parameters(&declared.return_type, &f.return_type)?;
            Some(f.rebuild(params, Vec::new(), Box::new(return_type)))
        }
        (
            Type::Compound { args: declared, .. },
            Type::Compound {
                kind,
                args,
                writable,
            },
        ) if declared.len() == args.len() => {
            let args = declared
                .iter()
                .zip(args)
                .map(|(declared, arg)| erase_at_parameters(declared, arg))
                .collect::<Option<Vec<_>>>()?;
            Some(Type::Compound {
                kind: *kind,
                args,
                writable: *writable,
            })
        }
        (
            Type::Nominal {
                params: declared, ..
            },
            Type::Nominal {
                id,
                params,
                writable,
            },
        ) if declared.len() == params.len() => {
            let params = declared
                .iter()
                .zip(params)
                .map(|(declared, param)| erase_at_parameters(declared, param))
                .collect::<Option<Vec<_>>>()?;
            Some(Type::Nominal {
                id: id.clone(),
                params,
                writable: *writable,
            })
        }
        (Type::Tuple(declared), Type::Tuple(elements)) if declared.len() == elements.len() => {
            let elements = declared
                .iter()
                .zip(elements)
                .map(|(declared, element)| erase_at_parameters(declared, element))
                .collect::<Option<Vec<_>>>()?;
            Some(Type::Tuple(elements))
        }
        (
            Type::Array {
                element: declared, ..
            },
            Type::Array { length, element },
        ) => Some(Type::Array {
            length: *length,
            element: Box::new(erase_at_parameters(declared, element)?),
        }),
        _ => Some(ty.clone()),
    }
}

fn signature_to_match(ty: &Type, clear_parameter_permissions: bool) -> Type {
    let Type::Function(function) = ty else {
        return ty.clone();
    };
    let parameters = if clear_parameter_permissions {
        function
            .params
            .iter()
            .map(|parameter| {
                parameter.with_type(without_function_bounds(&parameter.ty.without_permissions()))
            })
            .collect()
    } else {
        function.params.clone()
    };
    function.rebuild(parameters, vec![], function.return_type.clone())
}

fn without_function_bounds(ty: &Type) -> Type {
    let ty = ty.map_children(without_function_bounds);
    match ty {
        Type::Function(function) => function.rebuild(
            function.params.clone(),
            vec![],
            function.return_type.clone(),
        ),
        ty => ty,
    }
}

fn bound_requirement(store: &Store, bound: &Type, name: &str) -> Option<InterfaceRequirement> {
    interface_requirements(bound, |id| store.get_definition(id))
        .into_iter()
        .find(|candidate| candidate.name == name)
}
