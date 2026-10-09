use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::identifiers::method_expression;
use crate::names::go_name;
use crate::plan::values::GoExpression;
use syntax::program::DotAccessResolution;
use syntax::types::Type;

impl Planner<'_> {
    /// Constructor reference or unit variant call, e.g. `shapes.ShapeKind.CircleKind`.
    pub(crate) fn emit_enum_variant_dot(
        &mut self,
        definition: &str,
        result_ty: &Type,
    ) -> Option<GoExpression> {
        let make_fn_name = self.facts.variant_make_function(definition)?;
        let enum_package = self.facts.package_for_qualified_name(definition)?;
        let make_fn = if make_fn_name.starts_with(go_name::PRELUDE_PREFIX) {
            go_name::resolve(&make_fn_name).into_expression()
        } else if self.facts.is_current_package(enum_package) {
            GoExpression::name(make_fn_name)
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
                let type_args = if ret_params.len() > f.params.len() {
                    self.format_type_args(ret_params)
                } else {
                    String::new()
                };
                Some(GoExpression::instantiation(make_fn, type_args))
            }
            Type::Nominal { params, .. } => {
                let type_args = self.format_type_args(params);
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
        definition: &str,
        resolution: &DotAccessResolution,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let function = self.definition_reference(definition);
        if ctx.is_callee() {
            return function;
        }
        let type_args = self.format_value_type_args(Some(definition), resolution.instantiation());
        GoExpression::instantiation(function, type_args.unwrap_or_default())
    }

    /// Instance method used as a value (e.g. `lib.Point.area` callback →
    /// `lib.Point.Area` Go method expression).
    pub(crate) fn emit_instance_method_value_dot(
        &mut self,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        is_pointer_receiver: bool,
    ) -> Option<GoExpression> {
        let receiver = result_ty
            .as_function_type()?
            .params
            .first()?
            .ty
            .strip_refs();
        let receiver_type = self.use_go_type(&receiver);
        Some(method_expression(
            receiver_type,
            is_pointer_receiver,
            self.method_go_name(member, is_exported),
        ))
    }
}
