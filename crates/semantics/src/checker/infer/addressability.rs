use syntax::ast::{Expression, UnaryOperator};

use crate::checker::EnvResolve;
use crate::checker::TypeEnv;

pub(crate) fn check_is_non_addressable(
    expression: &Expression,
    env: &TypeEnv,
) -> Option<&'static str> {
    match expression {
        Expression::Identifier { .. } => None,
        Expression::DotAccess { expression, .. } => {
            let inner = expression.unwrap_parens();
            let is_non_addressable_origin = matches!(inner, Expression::StructCall { .. })
                || (matches!(inner, Expression::Call { .. })
                    && !expression.get_type().resolve_in(env).is_ref());
            if is_non_addressable_origin {
                Some("field access on non-addressable value")
            } else {
                check_is_non_addressable(expression, env)
            }
        }
        Expression::IndexedAccess { expression, .. } => {
            let expression_ty = expression.get_type().resolve_in(env);
            if let Some(name) = expression_ty.get_name() {
                if name == "Map" {
                    return Some("map index expression");
                }
                if name == "Slice" {
                    return None;
                }
            }
            if matches!(expression.unwrap_parens(), Expression::Call { .. }) {
                Some("index access on function call")
            } else {
                check_is_non_addressable(expression, env)
            }
        }
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => None,
        Expression::StructCall { .. } => None,
        Expression::Paren { expression, .. } => check_is_non_addressable(expression, env),
        Expression::Call { .. } => None,
        Expression::Literal { .. } => Some("literal"),
        Expression::Binary { .. } => Some("binary expression"),
        Expression::If { .. } | Expression::IfLet { .. } => Some("conditional expression"),
        Expression::Match { .. } => Some("match expression"),
        Expression::Block { .. } => Some("block expression"),
        Expression::Lambda { .. } => Some("lambda"),
        Expression::Tuple { .. } => Some("tuple"),
        Expression::Range { .. } => Some("range expression"),
        _ => Some("expression"),
    }
}

/// Check if an assignment target roots at a non-addressable expression.
/// Walks DotAccess/IndexedAccess chains to find the root. Call results,
/// struct literals, and tuple literals are not valid assignment roots.
pub(crate) fn check_non_addressable_assignment_target(
    expression: &Expression,
    env: &TypeEnv,
) -> Option<&'static str> {
    match expression.unwrap_parens() {
        Expression::Identifier { .. } => None,
        Expression::DotAccess { expression, .. } => {
            if matches!(expression.unwrap_parens(), Expression::Call { .. })
                && expression.get_type().resolve_in(env).is_ref()
            {
                None
            } else {
                check_non_addressable_assignment_target(expression, env)
            }
        }
        Expression::IndexedAccess { .. } => None,
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => None,
        Expression::Call { .. } => Some("function call result"),
        Expression::StructCall { .. } => Some("struct literal"),
        Expression::Tuple { .. } => Some("tuple literal"),
        _ => None,
    }
}
