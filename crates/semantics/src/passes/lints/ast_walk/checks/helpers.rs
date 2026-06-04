use syntax::ast::{BinaryOperator, Expression, Literal, Pattern};
use syntax::types::unqualified_name;

use crate::passes::walk::visit_ast;

pub(super) fn is_zero_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 0, .. },
            ..
        }
    )
}

pub(super) fn is_one_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 1, .. },
            ..
        }
    )
}

pub(super) fn flip_comparison(operator: BinaryOperator) -> BinaryOperator {
    use BinaryOperator::*;
    match operator {
        LessThan => GreaterThan,
        GreaterThan => LessThan,
        LessThanOrEqual => GreaterThanOrEqual,
        GreaterThanOrEqual => LessThanOrEqual,
        other => other,
    }
}

pub(super) fn bool_literal(expression: &Expression) -> Option<bool> {
    if let Expression::Literal {
        literal: Literal::Boolean(b),
        ..
    } = expression
    {
        Some(*b)
    } else {
        None
    }
}

pub(super) fn is_empty_block(expression: &Expression) -> bool {
    matches!(expression, Expression::Block { items, .. } if items.is_empty())
}

pub(super) fn is_side_effect_free(expression: &Expression) -> bool {
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

// The single identifier bound by a one-field enum-variant pattern such as
// `Some(v)` or `Err(e)`, if the variant name matches.
pub(super) fn enum_variant_binding<'a>(pattern: &'a Pattern, variant: &str) -> Option<&'a str> {
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = pattern
    else {
        return None;
    };
    if unqualified_name(identifier) != variant || *rest || fields.len() != 1 {
        return None;
    }
    let Pattern::Identifier {
        identifier: name, ..
    } = &fields[0]
    else {
        return None;
    };
    Some(name.as_str())
}

// `?`, `return`, `break`, and `continue` target a scope outside a synthesized
// closure, so a body containing them cannot be moved into one.
pub(super) fn has_escaping_control_flow(body: &Expression) -> bool {
    let mut found = false;
    visit_ast(
        std::slice::from_ref(body),
        &mut |node| {
            found |= matches!(
                node,
                Expression::Return { .. }
                    | Expression::Propagate { .. }
                    | Expression::Break { .. }
                    | Expression::Continue { .. }
            );
        },
        &mut |_| {},
    );
    found
}

pub(super) fn expressions_equivalent(a: &Expression, b: &Expression) -> bool {
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
        (Expression::Block { items: ai, .. }, Expression::Block { items: bi, .. }) => {
            ai.len() == bi.len() && ai.iter().zip(bi).all(|(x, y)| expressions_equivalent(x, y))
        }
        (
            Expression::Call {
                expression: ac,
                args: aa,
                ..
            },
            Expression::Call {
                expression: bc,
                args: ba,
                ..
            },
        ) => {
            expressions_equivalent(ac, bc)
                && aa.len() == ba.len()
                && aa.iter().zip(ba).all(|(x, y)| expressions_equivalent(x, y))
        }
        _ => false,
    }
}
