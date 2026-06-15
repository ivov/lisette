use rustc_hash::FxHashMap as HashMap;

use syntax::EcoString;
use syntax::ast::{Annotation, Generic, Span};
use syntax::program::DefinitionBody;
use syntax::types::{Symbol, Type, substitute, unqualified_name};

use super::TaskState;
use crate::checker::infer::expressions::comparison::bounds_conflict;
use crate::store::Store;

/// An impl bound resolved into the type's namespace: type-generic position, span, type.
type ImplBound = (usize, Span, Type);

/// Impl bounds and the receiver type's own bounds, both indexed by type-generic position.
type BoundsByPosition = (Vec<ImplBound>, Vec<Vec<Type>>);

impl TaskState<'_> {
    /// Reject an impl whose bound instantiates an interface the receiver type already bounds
    /// with different arguments. Returns whether a conflict was reported.
    pub(super) fn check_conflicting_impl_bounds(
        &mut self,
        store: &Store,
        receiver_annotation: &Annotation,
        receiver_qualified: &Symbol,
        impl_generics: &[Generic],
    ) -> bool {
        let Some((impl_bounds, type_bounds)) = self.resolve_impl_bounds_by_position(
            store,
            receiver_annotation,
            receiver_qualified,
            impl_generics,
        ) else {
            return false;
        };

        let type_label = unqualified_name(receiver_qualified.as_str());
        let mut found = false;
        for (position, span, impl_bound) in &impl_bounds {
            let Some(type_bounds_here) = type_bounds.get(*position) else {
                continue;
            };
            if bounds_conflict(store, type_bounds_here, impl_bound) {
                found = true;
                let interface = impl_bound
                    .get_qualified_id()
                    .map(unqualified_name)
                    .unwrap_or("");
                self.sink
                    .push(diagnostics::infer::impl_bound_conflicts_with_type(
                        interface, type_label, *span,
                    ));
            }
        }
        found
    }

    /// Reject two conditional impls that instantiate the same interface differently on one
    /// receiver type parameter, which the receiver-hoisting emit cannot represent.
    pub(super) fn check_conflicting_cross_impl_bounds(
        &mut self,
        store: &Store,
        receiver_annotation: &Annotation,
        receiver_qualified: &Symbol,
        impl_generics: &[Generic],
    ) {
        let Some((impl_bounds, _type_bounds)) = self.resolve_impl_bounds_by_position(
            store,
            receiver_annotation,
            receiver_qualified,
            impl_generics,
        ) else {
            return;
        };

        let type_label = unqualified_name(receiver_qualified.as_str());
        for (position, span, impl_bound) in &impl_bounds {
            let key: (EcoString, usize) = (receiver_qualified.as_str().into(), *position);
            let recorded = self.impl_param_interface_bounds.get(&key);
            // Re-registration guard: this exact bound (same source span) was already seen.
            if recorded.is_some_and(|seen| seen.iter().any(|(_, prev)| prev == span)) {
                continue;
            }
            let earlier = recorded.and_then(|seen| {
                seen.iter()
                    .find(|(ty, _)| bounds_conflict(store, std::slice::from_ref(ty), impl_bound))
                    .map(|(_, prev)| *prev)
            });
            if let Some(earlier) = earlier {
                let interface = impl_bound
                    .get_qualified_id()
                    .map(unqualified_name)
                    .unwrap_or("");
                self.sink.push(
                    diagnostics::infer::conflicting_impl_interface_instantiations(
                        interface, type_label, *span, earlier,
                    ),
                );
            }
            self.impl_param_interface_bounds
                .entry(key)
                .or_default()
                .push((impl_bound.clone(), *span));
        }
    }

    /// Resolve an impl block's bounds and its receiver type's bounds, both keyed by the
    /// receiver type's generic position, with impl parameters alpha-renamed into the type's
    /// namespace. `None` when there is nothing to compare.
    fn resolve_impl_bounds_by_position(
        &mut self,
        store: &Store,
        receiver_annotation: &Annotation,
        receiver_qualified: &Symbol,
        impl_generics: &[Generic],
    ) -> Option<BoundsByPosition> {
        if impl_generics.is_empty() {
            return None;
        }
        let type_generics = match store
            .get_definition(receiver_qualified.as_str())
            .map(|d| &d.body)
        {
            Some(
                DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. },
            ) => generics.clone(),
            _ => return None,
        };
        if type_generics.is_empty() {
            return None;
        }
        let Annotation::Constructor {
            params: receiver_args,
            ..
        } = receiver_annotation
        else {
            return None;
        };
        // An impl variable instantiates the type generic at its position in the receiver, so
        // rename impl variables to those generic names to put both sides in one namespace.
        let position_of = |name: &EcoString| {
            receiver_args.iter().position(|arg| {
                matches!(arg, Annotation::Constructor { name: n, params, .. }
                    if params.is_empty() && n == name)
            })
        };
        let alpha: HashMap<EcoString, Type> = impl_generics
            .iter()
            .filter_map(|ig| {
                let position = position_of(&ig.name)?;
                let type_param = type_generics.get(position)?.name.clone();
                Some((ig.name.clone(), Type::Parameter(type_param)))
            })
            .collect();

        self.scopes.push();
        self.put_in_scope(&type_generics);
        self.put_in_scope(impl_generics);
        let before = self.sink.len();

        let type_bounds: Vec<Vec<Type>> = type_generics
            .iter()
            .map(|generic| {
                generic
                    .bounds
                    .iter()
                    .filter_map(|bound| {
                        self.resolve_type_bound(store, bound, &generic.span, receiver_qualified)
                    })
                    .collect()
            })
            .collect();

        let mut impl_bounds: Vec<ImplBound> = Vec::new();
        for impl_generic in impl_generics {
            let Some(position) = position_of(&impl_generic.name) else {
                continue;
            };
            for bound in &impl_generic.bounds {
                if let Some(resolved) =
                    self.resolve_type_bound(store, bound, &impl_generic.span, receiver_qualified)
                {
                    impl_bounds.push((position, bound.get_span(), substitute(&resolved, &alpha)));
                }
            }
        }

        // Drop any noise resolution pushed for unresolvable bounds; conflict diagnostics are
        // reported by the caller, outside this window.
        self.sink.truncate(before);
        self.scopes.pop();

        Some((impl_bounds, type_bounds))
    }

    /// Resolve a type generic's bound annotation to a type, type arguments preserved so
    /// `Parent<string>` and `Parent<int>` stay distinct.
    fn resolve_type_bound(
        &mut self,
        store: &Store,
        bound: &Annotation,
        span: &Span,
        type_qualified: &Symbol,
    ) -> Option<Type> {
        let bound_ty = self.register_bound_annotation(store, bound, span);
        let id = match store.deep_resolve_alias(&bound_ty).get_qualified_id() {
            Some(id) => id.to_string(),
            None => self.resolve_type_bound_id(store, bound, span, type_qualified)?,
        };
        let type_module = store.module_for_qualified_name(type_qualified.as_str())?;
        // Arguments come from the annotation, so they survive even when
        // `register_bound_annotation` drops them or cannot resolve the bound.
        let params = match bound {
            Annotation::Constructor { params, .. } => params
                .iter()
                .map(|param| self.resolve_bound_arg(store, param, span, type_module))
                .collect(),
            _ => Vec::new(),
        };
        Some(Type::Nominal {
            id: id.into(),
            params,
            underlying_ty: None,
        })
    }

    /// Resolve a bound's type argument to a type. `convert_to_type` handles primitives,
    /// builtins, and in-scope parameters; an out-of-scope user type is resolved against the
    /// store instead so it carries the same `Nominal` the method side does.
    fn resolve_bound_arg(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
        type_module: &str,
    ) -> Type {
        let ty = self.convert_to_type(store, annotation, span);
        if !matches!(ty, Type::Error) {
            return ty;
        }
        let Annotation::Constructor { name, params, .. } = annotation else {
            return ty;
        };
        let Some(id) = resolve_definition_id(store, type_module, name) else {
            return ty;
        };
        let params = params
            .iter()
            .map(|param| self.resolve_bound_arg(store, param, span, type_module))
            .collect();
        Type::Nominal {
            id: id.into(),
            params,
            underlying_ty: None,
        }
    }

    /// Resolve a user interface bound's qualified id when it is out of scope: a bare name
    /// against the type's own module, or an `alias.Name` against the aliased module.
    fn resolve_type_bound_id(
        &mut self,
        store: &Store,
        bound: &Annotation,
        _span: &Span,
        type_qualified: &Symbol,
    ) -> Option<String> {
        let Annotation::Constructor { name, .. } = bound else {
            return None;
        };
        let type_module = store.module_for_qualified_name(type_qualified.as_str())?;
        let (bound_module, bound_name) = match name.split_once('.') {
            Some((alias, type_name)) => (
                import_module_for_alias(store, type_module, alias)?,
                type_name,
            ),
            None => (type_module.to_string(), name.as_str()),
        };
        let candidate = Symbol::from_parts(&bound_module, bound_name);
        matches!(
            store.get_definition(candidate.as_str()).map(|d| &d.body),
            Some(DefinitionBody::Interface { .. })
        )
        .then(|| candidate.to_string())
    }
}

/// Resolve a type name (any definition kind) to its qualified id out of file context:
/// a bare name against `type_module`, or an `alias.Name` against the aliased module.
fn resolve_definition_id(store: &Store, type_module: &str, name: &str) -> Option<String> {
    let (module, bare) = match name.split_once('.') {
        Some((alias, type_name)) => (
            import_module_for_alias(store, type_module, alias)?,
            type_name,
        ),
        None => (type_module.to_string(), name),
    };
    let candidate = Symbol::from_parts(&module, bare);
    store
        .get_definition(candidate.as_str())
        .map(|_| candidate.to_string())
}

/// The module id that `alias` imports within `module`, or `None`.
fn import_module_for_alias(store: &Store, module: &str, alias: &str) -> Option<String> {
    let module = store.get_module(module)?;
    module.files.values().find_map(|file| {
        file.imports().into_iter().find_map(|import| {
            (import.effective_alias(&store.go_package_names).as_deref() == Some(alias))
                .then(|| import.name.to_string())
        })
    })
}
