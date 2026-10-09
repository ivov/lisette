use crate::Planner;
use crate::calls::go_interop::build_tuple_literal;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::GoExpression;
use crate::state::bindings::BindingValue;
use syntax::ast::IdentifierResolution;
use syntax::types::FunctionParameter;
use syntax::types::{SubstitutionMap, Type, unqualified_name};

impl Planner<'_> {
    pub(crate) fn emit_identifier(
        &mut self,
        value: &str,
        resolution: &IdentifierResolution,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let binding = match resolution {
            IdentifierResolution::Definition {
                name,
                instantiation,
            } => {
                return self.emit_definition_identifier(name, instantiation, ty, ctx);
            }
            IdentifierResolution::Binding(id) => self.scope.resolve_binding_id(*id),
            // Unresolved names such as function-local consts bind by spelling.
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
            None => return self.resolve_go_name(value, resolution.binding_id().is_some()),
        };
        let mut go_name = self.resolve_go_name(local.spelling(), true);
        if let Some(id) = local.id()
            && let GoExpressionNode::Identifier(identifier) = go_name.node_mut()
            && identifier.spelling() == local.spelling()
        {
            identifier.identify(id);
        }
        go_name
    }

    fn emit_definition_identifier(
        &mut self,
        symbol: &str,
        instantiation: &SubstitutionMap,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        if let Some(make_function) = self.facts.variant_make_function(symbol) {
            match ty {
                Type::Nominal { params, .. } => {
                    let type_args = match ctx.expected_slot_type() {
                        Some(t) => self
                            .prelude_container_type_args(t)
                            .unwrap_or_else(|| self.format_type_args(params)),
                        None => self.format_type_args(params),
                    };
                    return GoExpression::pure_call(
                        GoExpression::instantiation(
                            self.resolve_go_name(&make_function, false),
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
                            self.resolve_go_name(&make_function, false),
                            type_args,
                        );
                    }
                }
                _ => {}
            }
        }

        if let Some(expression) = self.try_emit_method_expression(symbol, ty) {
            return expression;
        }
        let function = self.definition_reference(symbol);
        match self.value_type_args(symbol, instantiation, ctx) {
            Some(type_args) => GoExpression::instantiation(function, type_args),
            None => function,
        }
    }

    /// Type args for a generic definition used as a value.
    fn value_type_args(
        &mut self,
        symbol: &str,
        instantiation: &SubstitutionMap,
        ctx: ExpressionContext<'_>,
    ) -> Option<String> {
        if ctx.is_callee() {
            return None;
        }
        self.format_value_type_args(Some(symbol), Some(instantiation))
    }

    /// Type args for a constructor function reference (e.g. `MakeFoo[T]` used as a value).
    /// Skips type args when the callee position already supplies them or when they can be
    /// inferred from the parameter types.
    fn constructor_fn_type_args(
        &mut self,
        fn_params: &[FunctionParameter],
        ret_params: &[Type],
        ctx: ExpressionContext<'_>,
    ) -> String {
        let return_params_are_inferrable = ret_params.len() <= fn_params.len()
            && ret_params
                .iter()
                .all(|ret| fn_params.iter().any(|param| param.ty.contains_type(ret)));
        let parameters_support_go_inference = fn_params
            .iter()
            .all(|param| !self.is_function_alias(&param.ty));
        let inferred_at_call_site =
            ctx.is_callee() && return_params_are_inferrable && parameters_support_go_inference;
        if inferred_at_call_site {
            String::new()
        } else {
            self.format_type_args(ret_params)
        }
    }

    /// Go method-expression syntax for an instance method of a current-package type.
    fn try_emit_method_expression(&mut self, symbol: &str, id_ty: &Type) -> Option<GoExpression> {
        let package = self.facts.package_for_qualified_name(symbol)?;
        let (owner, method) = symbol[package.len() + 1..].rsplit_once('.')?;
        let owner_id = self.peel_alias_id(&format!("{package}.{owner}"));
        if !self
            .facts
            .package_for_qualified_name(&owner_id)
            .is_some_and(|package| self.facts.is_current_package(package))
        {
            return None;
        }

        let fn_params = match id_ty {
            Type::Function(f) => &f.params,
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Function(f) => &f.params,
                _ => return None,
            },
            _ => return None,
        };

        let first = fn_params.first()?;
        let stripped = first.ty.strip_refs();
        let is_self = matches!(stripped, Type::Nominal { ref id, .. } if id.as_str() == owner_id);
        if !is_self {
            return None;
        }

        let is_pointer = first.ty.is_ref();

        if self.facts.is_ufcs_method(&owner_id, method) {
            return None;
        }

        let is_public = self
            .facts
            .method(&owner_id, method)
            .map(|method| method.visibility.is_public())
            .unwrap_or(false);
        let go_method = self.method_go_name(method, is_public);

        let type_args = if let Type::Nominal { ref params, .. } = stripped {
            if params.is_empty() {
                String::new()
            } else {
                self.format_type_args(params)
            }
        } else {
            String::new()
        };

        let type_go = go_name::escape_type_name(unqualified_name(&owner_id));
        Some(method_expression(
            format!("{}{}", type_go, type_args),
            is_pointer,
            go_method,
        ))
    }
}

/// Go method expression on a type, `T.method` or `(*T).method`.
pub(crate) fn method_expression(
    receiver_type: String,
    pointer_receiver: bool,
    go_method: String,
) -> GoExpression {
    let receiver = if pointer_receiver {
        format!("(*{})", receiver_type)
    } else {
        receiver_type
    };
    GoExpression::selector(GoExpression::type_name(receiver), go_method)
}
