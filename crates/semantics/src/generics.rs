use ecow::EcoString;
use syntax::ast::Generic;
use syntax::types::{Type, build_substitution_map, substitute, unqualified_name};

use crate::checker::infer::BuiltinBound;
use crate::checker::infer::InferCtx;
use crate::checker::infer::expressions::comparison;
use crate::checker::{EnvResolve, TaskState};
use crate::facts::GenericBoundOrigin;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct AppliedGenericBound {
    pub parameter_name: EcoString,
    pub argument: Type,
    pub required: Type,
}

pub fn apply_bounds(generics: &[Generic], arguments: &[Type]) -> Vec<AppliedGenericBound> {
    let substitution = build_substitution_map(generics, arguments);
    generics
        .iter()
        .zip(arguments)
        .flat_map(|(generic, argument)| {
            let substitution = &substitution;
            generic
                .resolved_bounds
                .iter()
                .map(move |bound| AppliedGenericBound {
                    parameter_name: generic.name.clone(),
                    argument: argument.clone(),
                    required: substitute(bound, substitution),
                })
        })
        .collect()
}

/// Instantiate the bounds declared by a nominal type.
pub fn type_obligations(store: &Store, ty: &Type) -> Vec<AppliedGenericBound> {
    let resolved = store.deep_resolve_alias(ty);
    let Type::Nominal { id, params, .. } = &resolved else {
        return Vec::new();
    };
    let Some(generics) = store
        .get_definition(id)
        .and_then(|definition| definition.body.generics())
    else {
        return Vec::new();
    };
    apply_bounds(generics, params)
}

pub fn bound_implied(store: &Store, available: &[Type], required: &Type) -> bool {
    comparison::bound_implied(store, available, required)
}

pub fn bound_requires_evidence(store: &Store, bound: &Type) -> bool {
    let resolved = store.deep_resolve_alias(bound);
    resolved.get_qualified_id().is_some_and(|id| {
        BuiltinBound::from_qualified_id(id).is_some()
            || crate::checker::infer::interface::interface_requires_methods(store, id)
    })
}

pub fn bound_display_name(store: &Store, bound: &Type) -> EcoString {
    let resolved = store.deep_resolve_alias(bound);
    resolved.get_qualified_id().map_or_else(
        || resolved.to_string().into(),
        |id| unqualified_name(id).into(),
    )
}

impl TaskState<'_> {
    pub(crate) fn visible_parameter_bounds(&self) -> Vec<(EcoString, Vec<Type>)> {
        self.scopes
            .collect_all_trait_bounds()
            .into_iter()
            .map(|(parameter, bounds)| (parameter.last_segment().into(), bounds))
            .collect()
    }

    pub fn check_post_inference_bounds(&mut self, store: &Store) {
        self.check_pending_interface_bounds(store);
        self.check_resolved_generic_obligations(store);
    }

    fn check_resolved_generic_obligations(&mut self, store: &Store) {
        let obligations = std::mem::take(&mut self.facts.generic_bound_obligations);
        let mut unresolved = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        for obligation in obligations {
            let argument = store.deep_resolve_alias(&obligation.argument.resolve_in(&self.env));
            let required = store.deep_resolve_alias(&obligation.required.resolve_in(&self.env));
            let key = (obligation.span, argument.to_string(), required.to_string());
            if !seen.insert(key)
                || argument.contains_error()
                || argument.contains_unknown()
                || required.contains_error()
            {
                continue;
            }
            if argument.has_unbound_variables() {
                unresolved.push(obligation);
                continue;
            }
            if let GenericBoundOrigin::Construction {
                enclosing_return_type: Some(return_type),
                ..
            } = &obligation.origin
                && return_type_declares_obligation(store, return_type, &argument, &required)
            {
                continue;
            }
            if let Type::Parameter(parameter) = &argument {
                let available = obligation
                    .available_bounds
                    .iter()
                    .find(|(name, _)| name == parameter)
                    .map_or(&[][..], |(_, bounds)| bounds.as_slice());
                if !bound_implied(store, available, &required) {
                    let required_name = bound_display_name(store, &required);
                    self.sink.push(diagnostics::infer::missing_bound_on_param(
                        parameter,
                        &required_name,
                        obligation.span,
                    ));
                }
                continue;
            }

            if let Some(builtin) = required
                .get_qualified_id()
                .and_then(BuiltinBound::from_qualified_id)
            {
                self.check_builtin_bound_argument(store, &argument, builtin, obligation.span);
            } else {
                InferCtx::new(self, store).check_concrete_bound(
                    &argument,
                    &required,
                    &obligation.span,
                );
            }
        }

        self.facts.generic_bound_obligations = unresolved;
    }
}

fn return_type_declares_obligation(
    store: &Store,
    return_type: &Type,
    argument: &Type,
    required: &Type,
) -> bool {
    type_obligations(store, return_type)
        .into_iter()
        .any(|applied| {
            applied.argument == *argument && bound_implied(store, &[applied.required], required)
        })
}
