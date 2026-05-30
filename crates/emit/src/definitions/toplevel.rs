use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::ConstPlan;
use crate::plan::values::ValuePlan;
use crate::state::bindings::BindingValue;
use syntax::ast::{Expression, Generic, Literal, UnaryOperator};
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn emit_type_alias(
        &mut self,
        name: &str,
        generics: &[Generic],
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let is_fn_alias;
        let underlying = match ty {
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Nominal {
                    underlying_ty: Some(inner),
                    ..
                } if matches!(inner.as_ref(), Type::Function(_)) => {
                    is_fn_alias = true;
                    inner.as_ref()
                }
                other => {
                    is_fn_alias = false;
                    other
                }
            },
            Type::Nominal {
                underlying_ty: Some(inner),
                ..
            } if matches!(inner.as_ref(), Type::Function(_)) => {
                is_fn_alias = true;
                inner.as_ref()
            }
            _ => {
                is_fn_alias = false;
                ty
            }
        };
        let ty_string = self.go_type_string(underlying, fx);

        if let Type::Nominal { id, .. } = underlying
            && let Some(module) = self.facts.module_for_qualified_name(id.as_str())
            && !self.facts.is_current_module(module)
            && module != go_name::PRELUDE_MODULE
            && !go_name::is_go_import(module)
        {
            let module = module.to_string();
            self.require_module_import_fx(&module, fx);
        }

        let symbol = self.facts.qualified_current(name);
        let generics_string = self.generics_to_string_for_symbol(&symbol, generics, fx);

        let separator = if is_fn_alias { " " } else { " = " };
        format!(
            "type {}{}{}{}",
            go_name::escape_keyword(name),
            generics_string,
            separator,
            ty_string
        )
    }

    pub(crate) fn build_const_plan(
        &mut self,
        identifier: &str,
        expression: &Expression,
        ty: &Type,
        directive: String,
        fx: &mut EmitEffects,
    ) -> ConstPlan {
        let target_name = self
            .module
            .escape_remap(identifier)
            .unwrap_or(identifier)
            .to_string();
        let initial_go_name = self.scope.bind(identifier, target_name);
        let go_identifier = if self.try_declare(&initial_go_name) {
            initial_go_name
        } else {
            let fresh = self.fresh_var(Some(identifier));
            self.scope.bind(identifier, &fresh);
            self.try_declare(&fresh);
            fresh
        };
        let ty_str = self.go_type_string(ty, fx);

        // `is_go_const_eligible` admits only literals, identifiers, and
        // constexpr unary/binary — none of which carry setup statements.
        let raw_value = self.plan_value(expression, ExpressionContext::value(), fx);
        let value_text = raw_value.operand_text().unwrap_or_default().to_string();
        let value = if value_text.is_empty() {
            ValuePlan::Operand("struct{}{}".to_string())
        } else {
            ValuePlan::Operand(value_text)
        };
        let is_const = self.is_go_const_eligible(expression);
        if is_const {
            self.record_go_const(go_identifier.clone());
        }
        ConstPlan {
            directive,
            is_const,
            name: go_identifier,
            ty_str,
            value,
        }
    }

    pub(crate) fn emit_const(
        &mut self,
        identifier: &str,
        expression: &Expression,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let plan = self.build_const_plan(identifier, expression, ty, String::new(), fx);
        let mut out = String::new();
        Renderer.render_const_declaration(&mut out, &plan);
        out.trim_end_matches('\n').to_string()
    }

    fn is_go_const_eligible(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Literal { literal, .. } => matches!(
                literal,
                Literal::Integer { .. }
                    | Literal::Float { .. }
                    | Literal::Imaginary(_)
                    | Literal::Boolean(_)
                    | Literal::String { .. }
                    | Literal::Char(_)
            ),
            Expression::Identifier { value, .. } => {
                match self.scope.resolve_identifier_binding(value.as_str()) {
                    Some(BindingValue::GoName(name)) => self.is_go_const_binding(name),
                    Some(BindingValue::InlineExpr(_)) => false,
                    None => self.is_go_const_binding(value.as_str()),
                }
            }
            Expression::Binary { left, right, .. } => {
                self.is_go_const_eligible(left) && self.is_go_const_eligible(right)
            }
            Expression::Unary {
                operator: UnaryOperator::Negative | UnaryOperator::Not,
                expression,
                ..
            } => self.is_go_const_eligible(expression),
            _ => false,
        }
    }
}
