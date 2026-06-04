use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::If {
        condition,
        alternative,
        ..
    } = expression
        && let Expression::If {
            condition: next_condition,
            ..
        } = alternative.as_ref()
        && is_side_effect_free(condition)
        && is_side_effect_free(next_condition)
        && expressions_equivalent(condition, next_condition)
    {
        ctx.sink.push(diagnostics::infer::repeated_if_condition(
            &next_condition.get_span(),
        ));
    }
}

fn is_side_effect_free(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Identifier { .. } | Expression::Literal { .. } => true,
        Expression::Unary {
            expression: inner, ..
        } => is_side_effect_free(inner),
        Expression::Binary { left, right, .. } => {
            is_side_effect_free(left) && is_side_effect_free(right)
        }
        Expression::DotAccess {
            expression: inner, ..
        } => is_side_effect_free(inner),
        _ => false,
    }
}

fn expressions_equivalent(a: &Expression, b: &Expression) -> bool {
    let a = a.unwrap_parens();
    let b = b.unwrap_parens();
    match (a, b) {
        (Expression::Identifier { value: av, .. }, Expression::Identifier { value: bv, .. }) => {
            av == bv
        }
        (Expression::Literal { literal: al, .. }, Expression::Literal { literal: bl, .. }) => {
            al == bl
        }
        (
            Expression::Unary {
                operator: ao,
                expression: ae,
                ..
            },
            Expression::Unary {
                operator: bo,
                expression: be,
                ..
            },
        ) => ao == bo && expressions_equivalent(ae, be),
        (
            Expression::Binary {
                operator: ao,
                left: al,
                right: ar,
                ..
            },
            Expression::Binary {
                operator: bo,
                left: bl,
                right: br,
                ..
            },
        ) => ao == bo && expressions_equivalent(al, bl) && expressions_equivalent(ar, br),
        (
            Expression::DotAccess {
                expression: ae,
                member: am,
                ..
            },
            Expression::DotAccess {
                expression: be,
                member: bm,
                ..
            },
        ) => am == bm && expressions_equivalent(ae, be),
        _ => false,
    }
}
