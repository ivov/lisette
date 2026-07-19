use syntax::ast::{Generic, Span};
use syntax::types::{CompoundKind, SubstitutionMap, Type, substitute, unqualified_name};

use crate::checker::EnvResolve;
use crate::checker::TaskState;
use crate::checker::infer::expressions::comparison::bound_implied;
use crate::checker::infer::{BuiltinBound, InferCtx};
use crate::store::Store;

impl TaskState<'_> {
    pub(crate) fn check_transitive_generic_bounds(
        &mut self,
        store: &Store,
        own_generics: &[Generic],
        declaration_span: Span,
    ) {
        for generic in own_generics {
            for bound in &generic.resolved_bounds {
                self.check_bound_type(store, own_generics, declaration_span, bound, true, false);
            }
        }
    }

    pub(crate) fn check_value_position_bounds(
        &mut self,
        store: &Store,
        own_generics: &[Generic],
        types: &[(Type, Span)],
    ) {
        let mut seen = rustc_hash::FxHashSet::default();
        for (ty, span) in types {
            if seen.insert(ty.to_string()) {
                self.check_bound_type(store, own_generics, *span, ty, false, true);
            }
        }
    }

    fn check_bound_type(
        &mut self,
        store: &Store,
        own_generics: &[Generic],
        declaration_span: Span,
        ty: &Type,
        check_map_keys: bool,
        defer_to_interface_list: bool,
    ) {
        if let Type::Nominal { id, params, .. } = ty
            && !params.is_empty()
        {
            self.check_nominal_arguments(
                store,
                own_generics,
                declaration_span,
                id,
                params,
                defer_to_interface_list,
            );
        }
        if check_map_keys
            && let Type::Compound {
                kind: CompoundKind::Map,
                args,
            } = ty
            && let Some(key) = args.first()
        {
            self.check_map_key_comparable(store, key, declaration_span);
        }
        for child in type_argument_children(ty) {
            self.check_bound_type(
                store,
                own_generics,
                declaration_span,
                child,
                check_map_keys,
                defer_to_interface_list,
            );
        }
    }

    fn check_nominal_arguments(
        &mut self,
        store: &Store,
        own_generics: &[Generic],
        declaration_span: Span,
        referenced_id: &str,
        argument_types: &[Type],
        defer_to_interface_list: bool,
    ) {
        let Some(definition) = store.get_definition(referenced_id) else {
            return;
        };
        let referenced_generics = definition.body.generics().unwrap_or_default();
        let substitution: SubstitutionMap = referenced_generics
            .iter()
            .map(|generic| generic.name.clone())
            .zip(argument_types.iter().cloned())
            .collect();

        for (referenced_generic, argument_type) in referenced_generics.iter().zip(argument_types) {
            for declared in &referenced_generic.resolved_bounds {
                if declared.get_qualified_id() == Some(referenced_id) {
                    continue;
                }
                let required = substitute(declared, &substitution);
                if required.contains_error() {
                    continue;
                }
                let resolved_required = store.deep_resolve_alias(&required);
                if let Some(required_id) = resolved_required.get_qualified_id()
                    && store.get_interface(required_id).is_some()
                    && !crate::checker::infer::interface::interface_requires_methods(
                        store,
                        required_id,
                    )
                {
                    continue;
                }
                if defer_to_interface_list
                    && resolved_required
                        .get_qualified_id()
                        .and_then(BuiltinBound::from_qualified_id)
                        .is_some()
                {
                    continue;
                }
                let argument = store.deep_resolve_alias(&argument_type.resolve_in(&self.env));
                if let Type::Parameter(parameter_name) = &argument {
                    let Some(parameter_bounds) =
                        self.parameter_bounds(parameter_name, own_generics)
                    else {
                        continue;
                    };
                    if !bound_implied(store, &parameter_bounds, &required) {
                        let span = own_generics
                            .iter()
                            .find(|generic| generic.name == *parameter_name)
                            .map_or(declaration_span, |generic| generic.span);
                        self.sink.push(diagnostics::infer::missing_transitive_bound(
                            parameter_name,
                            &type_bound_display(&required),
                            unqualified_name(referenced_id),
                            span,
                        ));
                    }
                } else if let Some(required) = store
                    .deep_resolve_alias(&required)
                    .get_qualified_id()
                    .and_then(BuiltinBound::from_qualified_id)
                {
                    self.check_builtin_bound_argument(store, &argument, required, declaration_span);
                } else if !argument.contains_error()
                    && !argument.contains_unknown()
                    && !argument.is_variable()
                    && store
                        .deep_resolve_alias(&required)
                        .get_qualified_id()
                        .is_some_and(|id| store.get_interface(id).is_some())
                {
                    if defer_to_interface_list {
                        self.pending_interface_bound_checks.push((
                            argument,
                            required,
                            declaration_span,
                        ));
                    } else {
                        self.pending_generic_bound_checks.push((
                            argument,
                            required,
                            declaration_span,
                        ));
                    }
                }
            }
        }
    }

    pub fn check_pending_generic_bounds(&mut self, store: &Store) {
        let pending = std::mem::take(&mut self.pending_generic_bound_checks);
        let mut ctx = InferCtx::new(self, store);
        for (argument, required, span) in pending {
            ctx.check_concrete_bound(&argument, &required, &span);
        }
    }

    pub fn check_pending_interface_bounds(&mut self, store: &Store) {
        let pending = std::mem::take(&mut self.pending_interface_bound_checks);
        let mut seen = rustc_hash::FxHashSet::default();
        let mut ctx = InferCtx::new(self, store);
        for (argument, required, span) in pending {
            if seen.insert((span, argument.to_string(), required.to_string())) {
                ctx.check_concrete_bound(&argument, &required, &span);
            }
        }
    }

    fn parameter_bounds(
        &self,
        parameter_name: &str,
        own_generics: &[Generic],
    ) -> Option<Vec<Type>> {
        if self.scopes.lookup_type_param(parameter_name).is_some() {
            let mut bounds = Vec::new();
            self.scopes
                .for_each_bound_on_param(parameter_name, |bound| {
                    bounds.push(bound.resolve_in(&self.env));
                });
            return (!bounds.iter().any(Type::contains_error)).then_some(bounds);
        }
        let generic = own_generics
            .iter()
            .find(|generic| generic.name == parameter_name)?;
        (!generic.resolved_bounds.iter().any(Type::contains_error))
            .then(|| generic.resolved_bounds.clone())
    }
}

fn type_argument_children(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Nominal { params, .. } => params.iter().collect(),
        Type::Compound { args, .. } => args.iter().collect(),
        Type::Array { element, .. } => vec![element],
        Type::Tuple(elements) => elements.iter().collect(),
        Type::Function(function) => function
            .params
            .iter()
            .chain(std::iter::once(function.return_type.as_ref()))
            .collect(),
        _ => Vec::new(),
    }
}

fn type_bound_display(ty: &Type) -> String {
    match ty {
        Type::Nominal { id, params, .. } => {
            let name = unqualified_name(id).to_string();
            if params.is_empty() {
                name
            } else {
                let arguments = params
                    .iter()
                    .map(type_bound_display)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{arguments}>")
            }
        }
        Type::Parameter(name) => name.to_string(),
        other => other.to_string(),
    }
}
