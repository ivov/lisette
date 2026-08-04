use syntax::ast::Span;
use syntax::types::{Bound, Type, unqualified_name};

use crate::checker::infer::InferCtx;
use crate::facts::{GenericBoundObligation, GenericBoundOrigin};
use crate::generics::{AppliedGenericBound, bound_requires_evidence, type_obligations};

impl InferCtx<'_> {
    pub(crate) fn register_construction_obligations(
        &mut self,
        written_name: &str,
        constructed_ty: &Type,
        span: Span,
    ) {
        let has_equals_method = constructed_ty
            .get_qualified_id()
            .is_some_and(|id| self.store.definition_has_equals_method(id));
        let origin = GenericBoundOrigin::Construction {
            name: unqualified_name(written_name).into(),
            enclosing_return_type: self.scopes.lookup_fn_return_type().cloned(),
            has_equals_method,
        };
        for bound in type_obligations(self.store, constructed_ty) {
            self.register_generic_bound_obligation(bound, &origin, span);
        }
    }

    pub(crate) fn register_function_value_obligations(
        &mut self,
        written_name: &str,
        function_ty: &Type,
        span: Span,
    ) {
        let origin = GenericBoundOrigin::FunctionReference {
            name: unqualified_name(written_name).into(),
        };
        for bound in function_ty.get_bounds() {
            self.register_generic_bound_obligation(applied_function_bound(bound), &origin, span);
        }
    }

    fn register_generic_bound_obligation(
        &mut self,
        bound: AppliedGenericBound,
        origin: &GenericBoundOrigin,
        span: Span,
    ) {
        if !bound_requires_evidence(self.store, &bound.required) {
            return;
        }
        let package_id = self.cursor.package_id.clone();
        let available_bounds = self.visible_parameter_bounds();
        self.facts
            .deferred
            .generic_bounds
            .push(GenericBoundObligation {
                argument: bound.argument,
                required: bound.required,
                span,
                package_id,
                param_name: bound.parameter_name,
                available_bounds,
                origin: origin.clone(),
            });
    }
}

fn applied_function_bound(bound: &Bound) -> AppliedGenericBound {
    AppliedGenericBound {
        parameter_name: bound.param_name.clone(),
        argument: bound.generic.clone(),
        required: bound.ty.clone(),
    }
}
