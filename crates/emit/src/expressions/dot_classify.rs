use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::identifiers::method_expression;
use crate::names::go_name;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use syntax::ast::Expression;
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
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        is_pointer_receiver: bool,
    ) -> Option<GoExpression> {
        if let Expression::Identifier { value, .. } = expression {
            let go_method = self.method_go_name(member, is_exported);
            let type_name = self
                .resolve_alias_type_name(value)
                .unwrap_or_else(|| value.to_string());
            let type_go = go_name::escape_type_name(&type_name);
            let type_args = self.method_expression_type_args(result_ty);
            return Some(method_expression(
                format!("{}{}", type_go, type_args),
                is_pointer_receiver,
                go_method,
            ));
        }

        let Expression::DotAccess {
            expression: inner_expression,
            member: type_name,
            ..
        } = expression
        else {
            return None;
        };

        let inner_ty = inner_expression.get_type();

        let package_name = if let Some(synthetic_package) = inner_ty.as_import_namespace() {
            synthetic_package.to_string()
        } else if matches!(&inner_ty, Type::Nominal { .. })
            && let Expression::Identifier { value, .. } = inner_expression.as_ref()
        {
            value.to_string()
        } else {
            return None;
        };
        let package_name = package_name.as_str();

        let go_method = self.method_go_name(member, is_exported);

        let package = self.package_use_for_package(package_name);
        let go_type_name = go_name::snake_to_camel(type_name);
        let type_args = self.method_expression_type_args(result_ty);
        let receiver_type = format!("{}.{}{}", package.qualifier(), go_type_name, type_args);
        let receiver_type = self.use_rendered_go_type(GoType::with_package(receiver_type, package));

        Some(method_expression(
            receiver_type,
            is_pointer_receiver,
            go_method,
        ))
    }

    fn method_expression_type_args(&mut self, result_ty: &Type) -> String {
        let Some(f) = result_ty.as_function_type() else {
            return String::new();
        };
        let Some(first_param) = f.params.first() else {
            return String::new();
        };
        let Type::Nominal {
            params: receiver_params,
            ..
        } = first_param.ty.strip_refs()
        else {
            return String::new();
        };
        if receiver_params.is_empty() {
            String::new()
        } else {
            self.format_type_args(&receiver_params)
        }
    }
}
