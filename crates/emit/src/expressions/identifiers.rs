use crate::Planner;
use crate::calls::go_interop::build_tuple_literal;
use crate::ir::values::GoExpression;
use crate::names::go_name;
use crate::state::bindings::BindingValue;
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use syntax::ast::{Expression, IdentifierResolution};
use syntax::program::CallOperation;
use syntax::types::FunctionParameter;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn plan_identifier(
        &mut self,
        identifier: &Expression,
        value: &str,
        resolution: &IdentifierResolution,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let binding = match resolution {
            IdentifierResolution::Definition { name, .. } => {
                return self.plan_definition_identifier(identifier, name, ty, ctx);
            }
            IdentifierResolution::Binding(id) => self.scope.resolve_binding_id(*id),

            IdentifierResolution::Unresolved => self.scope.resolve_identifier_binding(value),
        };
        let local = match binding {
            Some(BindingValue::InlineExpr(expr)) => return expr.expression().clone(),
            Some(BindingValue::Components(components)) => {
                let components = components.clone();
                return self.rebuild_from_components(&components);
            }
            Some(BindingValue::TupleComponents(tuple)) => {
                return build_tuple_literal(
                    tuple
                        .names
                        .iter()
                        .cloned()
                        .map(GoExpression::identifier)
                        .collect(),
                );
            }
            Some(BindingValue::GoName(name) | BindingValue::GoConst(name)) => name.clone(),
            None => {
                assert!(
                    resolution.binding_id().is_none(),
                    "source binding has no emitted value: {value}"
                );
                return self.facts.resolve_go_name(value);
            }
        };
        GoExpression::identifier(local)
    }

    fn plan_definition_identifier(
        &mut self,
        identifier: &Expression,
        symbol: &str,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        if let Some(make_function) = self.facts.variant_make_function(symbol) {
            match ty {
                Type::Nominal { params, .. } => {
                    let type_args = self.variant_value_type_args(params, ctx);
                    return GoExpression::pure_call(
                        GoExpression::instantiation(
                            self.facts.resolve_go_name(&make_function),
                            type_args,
                        ),
                        Vec::new(),
                    );
                }
                Type::Function(f) => {
                    if let Type::Nominal {
                        params: ret_params, ..
                    } = f.return_type.as_ref()
                    {
                        let type_args = self.constructor_fn_type_args(&f.params, ret_params, ctx);
                        return GoExpression::instantiation(
                            self.facts.resolve_go_name(&make_function),
                            type_args,
                        );
                    }
                }
                _ => {}
            }
        }

        if let Some(expression) = self.try_plan_method_expression(identifier, ty) {
            return expression;
        }
        let function = self.definition_reference(symbol);
        if ctx.is_callee() {
            return function;
        }
        match self.format_value_type_args(identifier) {
            Some(type_args) => GoExpression::instantiation(function, type_args),
            None => function,
        }
    }

    pub(super) fn variant_value_type_args(
        &mut self,
        params: &[Type],
        ctx: ExpressionContext<'_>,
    ) -> GoType {
        ctx.expected_slot_type()
            .and_then(|t| self.prelude_container_type_args(t))
            .unwrap_or_else(|| self.format_type_args(params))
    }

    pub(super) fn constructor_fn_type_args(
        &mut self,
        fn_params: &[FunctionParameter],
        ret_params: &[Type],
        ctx: ExpressionContext<'_>,
    ) -> GoType {
        let return_params_are_inferrable = ret_params.len() <= fn_params.len()
            && ret_params
                .iter()
                .all(|ret| fn_params.iter().any(|param| param.ty.contains_type(ret)));
        let parameters_support_go_inference = fn_params
            .iter()
            .all(|param| !self.facts.is_function_alias(&param.ty));
        let inferred_at_call_site =
            ctx.is_callee() && return_params_are_inferrable && parameters_support_go_inference;
        if inferred_at_call_site {
            GoType::default()
        } else {
            self.format_type_args(ret_params)
        }
    }

    fn try_plan_method_expression(
        &mut self,
        identifier: &Expression,
        id_ty: &Type,
    ) -> Option<GoExpression> {
        let recorded = self.facts.checked(identifier).function_value()?;
        let CallOperation::ReceiverMethod { is_public } = recorded.operation else {
            return None;
        };
        let symbol = recorded.definition.as_deref()?;
        let (_, method) = symbol.rsplit_once('.')?;
        let first = id_ty.as_function_type()?.params.first()?;
        let mut go_method = self.facts.method_go_name(method, is_public);
        if self.facts.definition_selects_tagged_twin(symbol) {
            go_method = go_name::tagged_twin(&go_method);
        }
        Some(self.method_expression(&first.ty, go_method))
    }

    pub(crate) fn method_expression(
        &self,
        receiver_param: &Type,
        go_method: String,
    ) -> GoExpression {
        let receiver_type = self.go_type(&receiver_param.strip_refs());
        let receiver = if receiver_param.is_ref() {
            GoType::pointer_to(&receiver_type).parenthesized()
        } else {
            receiver_type
        };
        GoExpression::selector(GoExpression::type_name(receiver), go_method)
    }
}
