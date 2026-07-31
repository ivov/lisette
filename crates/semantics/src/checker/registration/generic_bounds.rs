use syntax::ast::{Generic, Span};
use syntax::types::{CompoundKind, Type, unqualified_name};

use crate::checker::EnvResolve;
use crate::checker::TaskState;
use crate::checker::infer::{BuiltinBound, InferCtx};
use crate::generics::{apply_bounds, bound_implied, type_argument_children};
use crate::store::Store;

#[derive(Clone, Copy)]
enum BoundCheckContext {
    Declaration,
    Value,
}

#[derive(Clone, Copy)]
struct BoundsCheckSite<'a> {
    store: &'a Store,
    own_generics: &'a [Generic],
    declaration_span: Span,
}

impl TaskState {
    pub(crate) fn check_transitive_generic_bounds(
        &mut self,
        store: &Store,
        own_generics: &[Generic],
        declaration_span: Span,
    ) {
        let site = BoundsCheckSite {
            store,
            own_generics,
            declaration_span,
        };
        for generic in own_generics {
            for bound in generic
                .resolved_bounds()
                .expect("generic bounds were resolved before checking")
            {
                self.check_bound_type(&site, bound, BoundCheckContext::Declaration);
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
                let site = BoundsCheckSite {
                    store,
                    own_generics,
                    declaration_span: *span,
                };
                self.check_bound_type(&site, ty, BoundCheckContext::Value);
            }
        }
    }

    fn check_bound_type(
        &mut self,
        site: &BoundsCheckSite<'_>,
        ty: &Type,
        context: BoundCheckContext,
    ) {
        if let Type::Nominal { id, params, .. } = ty
            && !params.is_empty()
        {
            self.check_nominal_arguments(site, id, params, context);
        }
        if matches!(context, BoundCheckContext::Declaration)
            && let Type::Compound {
                kind: CompoundKind::Map,
                args,
            } = ty
            && let Some(key) = args.first()
        {
            self.check_map_key_comparable(site.store, key, site.declaration_span);
        }
        for child in type_argument_children(ty) {
            self.check_bound_type(site, child, context);
        }
    }

    fn check_nominal_arguments(
        &mut self,
        site: &BoundsCheckSite<'_>,
        referenced_id: &str,
        argument_types: &[Type],
        context: BoundCheckContext,
    ) {
        let BoundsCheckSite {
            store,
            own_generics,
            declaration_span,
        } = *site;
        let Some(definition) = store.get_definition(referenced_id) else {
            return;
        };
        let referenced_generics = definition.body.generics().unwrap_or_default();
        for applied in apply_bounds(referenced_generics, argument_types) {
            let required = applied.required;
            if required.get_qualified_id() == Some(referenced_id) || required.contains_error() {
                continue;
            }
            let resolved_required = store.deep_resolve_alias(&required);
            if let Some(required_id) = resolved_required.get_qualified_id()
                && store.get_interface(required_id).is_some()
                && !crate::checker::infer::interface::interface_requires_methods(store, required_id)
            {
                continue;
            }
            if matches!(context, BoundCheckContext::Value)
                && resolved_required
                    .get_qualified_id()
                    .and_then(BuiltinBound::from_qualified_id)
                    .is_some()
            {
                continue;
            }
            let argument = store.deep_resolve_alias(&applied.argument.resolve_in(&self.env));
            if let Type::Parameter(parameter_name) = &argument {
                let Some(parameter_bounds) = self.parameter_bounds(parameter_name, own_generics)
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
            } else if let Some(required) = resolved_required
                .get_qualified_id()
                .and_then(BuiltinBound::from_qualified_id)
            {
                self.check_builtin_bound_argument(
                    store,
                    &argument,
                    required,
                    declaration_span,
                    None,
                );
            } else if !argument.contains_error()
                && !store.contains_unknown(&argument)
                && !argument.is_variable()
                && resolved_required
                    .get_qualified_id()
                    .is_some_and(|id| store.get_interface(id).is_some())
            {
                match context {
                    BoundCheckContext::Declaration => self.pending_generic_bound_checks.push((
                        argument,
                        required,
                        declaration_span,
                    )),
                    BoundCheckContext::Value => self.pending_interface_bound_checks.push((
                        argument,
                        required,
                        declaration_span,
                    )),
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

    pub(crate) fn check_pending_interface_bounds(&mut self, store: &Store) {
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
        let bounds = generic.resolved_bounds()?.cloned().collect::<Vec<_>>();
        (!bounds.iter().any(Type::contains_error)).then_some(bounds)
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
