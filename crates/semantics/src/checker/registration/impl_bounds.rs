use rustc_hash::FxHashMap as HashMap;

use syntax::EcoString;
use syntax::ast::{Annotation, Generic, Span};
use syntax::program::DefinitionBody;
use syntax::types::{Bound, Symbol, Type, substitute, unqualified_name};

use super::TaskState;
use crate::checker::infer::expressions::comparison::bound_implied;
use crate::store::Store;

impl TaskState<'_> {
    /// Make receiver-declaration bounds available inside a regular impl. The impl may state
    /// weaker bounds, but every valid receiver instantiation still satisfies these bounds.
    pub(crate) fn register_receiver_type_bounds(
        &mut self,
        store: &Store,
        receiver_qualified: &Symbol,
        impl_generics: &[Generic],
    ) -> Vec<Vec<Type>> {
        let type_generics = match store
            .get_definition(receiver_qualified.as_str())
            .map(|definition| &definition.body)
        {
            Some(
                DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. },
            ) => generics.clone(),
            _ => return Vec::new(),
        };
        let alpha: HashMap<EcoString, Type> = type_generics
            .iter()
            .zip(impl_generics)
            .map(|(type_generic, impl_generic)| {
                (
                    type_generic.name.clone(),
                    Type::Parameter(impl_generic.name.clone()),
                )
            })
            .collect();

        self.scopes.push();
        self.put_in_scope(&type_generics);
        let before = self.sink.len();
        let mut bounds_by_position = Vec::with_capacity(type_generics.len());
        for generic in &type_generics {
            let mut resolved_bounds = Vec::with_capacity(generic.bounds.len());
            for annotation in &generic.bounds {
                let bound = self
                    .facts
                    .bound_types
                    .get(&annotation.get_span())
                    .cloned()
                    .or_else(|| {
                        self.resolve_type_bound(
                            store,
                            annotation,
                            &generic.span,
                            receiver_qualified,
                        )
                    });
                if let Some(bound) = bound
                    && !bound.contains_error()
                {
                    resolved_bounds.push(substitute(&bound, &alpha));
                }
            }
            bounds_by_position.push(resolved_bounds);
        }
        self.sink.truncate(before);
        self.scopes.pop();

        for (generic, bounds) in impl_generics.iter().zip(&bounds_by_position) {
            for bound in bounds {
                self.record_generic_bound(&generic.name, bound.clone());
            }
        }
        bounds_by_position
    }

    /// Reject bounds that are not already guaranteed by the receiver type's declaration.
    pub(super) fn check_strengthened_impl_bounds(
        &mut self,
        store: &Store,
        receiver_qualified: &Symbol,
        impl_generics: &[Generic],
        impl_bounds: &[Bound],
        receiver_bounds: &[Vec<Type>],
    ) {
        if impl_bounds.is_empty() {
            return;
        }
        let mut resolved_bounds = impl_bounds.iter();
        for (position, generic) in impl_generics.iter().enumerate() {
            for annotation in &generic.bounds {
                let Some(bound) = resolved_bounds.next() else {
                    return;
                };
                let Some(type_bounds) = receiver_bounds.get(position) else {
                    continue;
                };
                let impl_bound = bound.ty.clone();
                if impl_bound.contains_error() {
                    continue;
                }
                if !bound_implied(store, type_bounds, &impl_bound) {
                    self.sink
                        .push(diagnostics::infer::impl_bound_strengthens_type(
                            unqualified_name(receiver_qualified.as_str()),
                            annotation.get_span(),
                        ));
                }
            }
        }
    }

    /// Resolve a type generic's bound annotation to a type, type arguments preserved so
    /// `Parent<string>` and `Parent<int>` stay distinct.
    pub(super) fn resolve_type_bound(
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
pub(super) fn import_module_for_alias(store: &Store, module: &str, alias: &str) -> Option<String> {
    let module = store.get_module(module)?;
    module.files.values().find_map(|file| {
        file.imports().into_iter().find_map(|import| {
            (import.effective_alias(&store.go_package_names).as_deref() == Some(alias))
                .then(|| import.name.to_string())
        })
    })
}
