use crate::Planner;
use crate::lookup::state::bindings::BindingValue;
use crate::lowering::calls::go_interop::build_tuple_literal;
use crate::lowering::definitions::interface_adapter::definition_selects_tagged_twin;
use crate::lowering::plan::context::ExpressionContext;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, IdentifierResolution};
use syntax::program::{CallOperation, strip_prelude_prefix};
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn plan_identifier(
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
                return resolve(value);
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
        if let Some(make_function) = self.variant_make_reference(symbol) {
            match ty {
                Type::Nominal { params, .. } => {
                    let type_args = self.variant_value_type_args(params, ctx);
                    return GoExpression::pure_call(
                        GoExpression::instantiation(make_function, type_args),
                        Vec::new(),
                    );
                }
                Type::Function(f) => {
                    if let Type::Nominal {
                        params: ret_params, ..
                    } = f.return_type.as_ref()
                    {
                        let type_args =
                            self.constructor_reference_type_arguments(&f.params, ret_params, ctx);
                        return GoExpression::instantiation(make_function, type_args);
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
        let go_method = self.facts.receiver_method_go_name(
            &first.ty,
            method,
            is_public,
            definition_selects_tagged_twin(&self.facts, symbol),
        );
        Some(self.method_expression(&first.ty, go_method))
    }

    pub(in crate::lowering) fn method_expression(
        &self,
        receiver_param: &Type,
        go_method: String,
    ) -> GoExpression {
        let receiver_type = self.go_type(&self.facts.strip_refs(receiver_param));
        let receiver = if self.facts.is_reference(receiver_param) {
            GoType::pointer_to(&receiver_type).parenthesized()
        } else {
            receiver_type
        };
        GoExpression::selector(GoExpression::type_name(receiver), go_method)
    }
}

fn resolve(name: &str) -> GoExpression {
    if let Some(rest) = strip_prelude_prefix(name) {
        let go_name: String = rest.split('.').map(go_name::snake_to_camel).collect();
        GoExpression::qualified(PackageUse::generated(GeneratedPackage::Prelude), go_name)
    } else {
        GoExpression::external_name(go_name::escape_reserved(&name.replace('.', "_")).into_owned())
    }
}
