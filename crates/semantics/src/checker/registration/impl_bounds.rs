use rustc_hash::FxHashMap as HashMap;

use syntax::EcoString;
use syntax::ast::Generic;
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

        let mut bounds_by_position = Vec::with_capacity(type_generics.len());
        for generic in &type_generics {
            let resolved_bounds: Vec<Type> = generic
                .resolved_bounds
                .iter()
                .filter(|bound| !bound.contains_error())
                .map(|bound| substitute(bound, &alpha))
                .collect();
            bounds_by_position.push(resolved_bounds);
        }

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
}
