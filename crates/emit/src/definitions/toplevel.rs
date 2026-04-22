use crate::Emitter;
use crate::names::go_name;
use syntax::ast::{Expression, Generic, UnaryOperator};
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_type_alias(
        &mut self,
        name: &str,
        generics: &[Generic],
        ty: &Type,
    ) -> String {
        let is_fn_alias;
        let underlying = match ty {
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Nominal {
                    underlying_ty: Some(inner),
                    ..
                } if matches!(inner.as_ref(), Type::Function { .. }) => {
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
            } if matches!(inner.as_ref(), Type::Function { .. }) => {
                is_fn_alias = true;
                inner.as_ref()
            }
            _ => {
                is_fn_alias = false;
                ty
            }
        };
        let ty_string = self.go_type_as_string(underlying);

        if let Type::Nominal { id, .. } = underlying
            && let Some((module, _)) = id.split_once('.')
            && module != self.current_module
            && module != go_name::PRELUDE_MODULE
            && !go_name::is_go_import(module)
        {
            self.require_module_import(module);
        }

        let generic_names: Vec<&str> = generics.iter().map(|g| g.name.as_ref()).collect();
        let map_key_generics =
            Self::collect_map_key_generics(std::iter::once(underlying), &generic_names);
        let generics_string = self.generics_to_string_with_map_keys(generics, &map_key_generics);

        let separator = if is_fn_alias { " " } else { " = " };
        format!(
            "type {}{}{}{}",
            go_name::escape_keyword(name),
            generics_string,
            separator,
            ty_string
        )
    }

    pub(crate) fn emit_const(
        &mut self,
        identifier: &str,
        expression: &Expression,
        ty: &Type,
    ) -> String {
        let go_identifier = self.scope.bindings.add(identifier, identifier);
        let ty_str = self.go_type_as_string(ty);

        let mut output = String::new();
        let expression_string = self.emit_operand(&mut output, expression);
        let value = if expression_string.is_empty() {
            "struct{}{}"
        } else {
            &expression_string
        };
        let keyword = if Self::is_go_const_eligible(expression) {
            "const"
        } else {
            "var"
        };
        format!("{} {} {} = {}", keyword, go_identifier, ty_str, value)
    }

    fn is_go_const_eligible(expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Literal { .. } => true,
            Expression::Binary { left, right, .. } => {
                Self::is_go_const_eligible(left) && Self::is_go_const_eligible(right)
            }
            Expression::Unary {
                operator: UnaryOperator::Negative | UnaryOperator::Not,
                expression,
                ..
            } => Self::is_go_const_eligible(expression),
            _ => false,
        }
    }
}
