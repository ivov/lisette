use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::values::GoExpression;
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn emit_enum_variant_dot(
        &mut self,
        definition: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        let make_fn_name = self.facts.variant_make_function(definition)?;
        let enum_package = self.facts.package_for_qualified_name(definition)?;
        let make_fn = if go_name::is_prelude_id(&make_fn_name) {
            go_name::resolve(&make_fn_name).into_expression()
        } else if self.facts.is_current_package(enum_package) {
            GoExpression::external_name(make_fn_name)
        } else {
            GoExpression::qualified(self.package_use_for_package(enum_package), make_fn_name)
        };

        match result_ty {
            Type::Function(f) => {
                let Type::Nominal {
                    params: ret_params, ..
                } = f.return_type.as_ref()
                else {
                    return None;
                };
                let type_args = self.constructor_fn_type_args(&f.params, ret_params, ctx);
                Some(GoExpression::instantiation(make_fn, type_args))
            }
            Type::Nominal { params, .. } => {
                let type_args = self.variant_value_type_args(params, ctx);
                Some(GoExpression::pure_call(
                    GoExpression::instantiation(make_fn, type_args),
                    Vec::new(),
                ))
            }
            _ => None,
        }
    }

    pub(crate) fn emit_static_method_dot(
        &mut self,
        dot_access: &Expression,
        definition: &str,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let function = self.definition_reference(definition);
        if ctx.is_callee() {
            return function;
        }
        let type_args = self.format_value_type_args(dot_access);
        GoExpression::instantiation(function, type_args.unwrap_or_default())
    }

    pub(crate) fn emit_instance_method_value_dot(
        &mut self,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        tagged_twin: bool,
    ) -> Option<GoExpression> {
        let receiver_param = &result_ty.as_function_type()?.params.first()?.ty;
        let mut go_method = self.method_go_name(member, is_exported);
        if tagged_twin {
            go_method = go_name::tagged_twin(&go_method);
        }
        Some(self.method_expression(receiver_param, go_method))
    }
}
