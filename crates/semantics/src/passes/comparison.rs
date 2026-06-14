use syntax::ast::{BinaryOperator, Expression, UnaryOperator};
use syntax::types::SimpleKind;

/// A value bound: `(value, inclusive)`, or `None` for an open side.
pub(crate) type Bound = Option<(i128, bool)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaskOp {
    And,
    Or,
}

/// A masked integer comparison, `operator` flipped so the masked side is left.
pub(crate) struct MaskComparison {
    pub(crate) mask_op: MaskOp,
    pub(crate) mask: i128,
    pub(crate) operator: BinaryOperator,
    pub(crate) constant: i128,
    pub(crate) kind: SimpleKind,
}

/// Decomposes a masked integer comparison; `None` unless `m` and `c` are in-range
/// integer literals and `m != 0` (zero masks belong to `redundant_operation`).
/// A negative `m` is valid only for the equality reasoning.
pub(crate) fn mask_comparison(expression: &Expression) -> Option<MaskComparison> {
    use BinaryOperator::*;
    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = expression
    else {
        return None;
    };
    if !matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    ) {
        return None;
    }

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();
    let (mask_expr, operator, constant) =
        match (signed_integer_literal(left), signed_integer_literal(right)) {
            (None, Some(constant)) => (left, *operator, constant),
            (Some(constant), None) => (right, flip_comparison(*operator), constant),
            _ => return None,
        };

    let Expression::Binary {
        operator: bit_operator,
        left: bit_left,
        right: bit_right,
        ..
    } = mask_expr
    else {
        return None;
    };
    let mask_op = match bit_operator {
        BitwiseAnd => MaskOp::And,
        BitwiseOr => MaskOp::Or,
        _ => return None,
    };
    let bit_left = bit_left.unwrap_parens();
    let bit_right = bit_right.unwrap_parens();
    let mask = match (
        signed_integer_literal(bit_left),
        signed_integer_literal(bit_right),
    ) {
        (None, Some(mask)) | (Some(mask), None) => mask,
        _ => return None,
    };

    let kind = mask_expr.get_type().underlying_simple_kind()?;
    if !kind.is_ordered() {
        return None;
    }
    let (min, max) = kind.integer_range()?;
    if mask == 0 || mask < min || mask > max || constant < min || constant > max {
        return None;
    }

    Some(MaskComparison {
        mask_op,
        mask,
        operator,
        constant,
        kind,
    })
}

pub(crate) fn in_scope_comparison(a: &Expression, b: &Expression) -> bool {
    match (signed_integer_literal(a), signed_integer_literal(b)) {
        (None, Some(value)) => integer_operand_fits(a, value),
        (Some(value), None) => integer_operand_fits(b, value),
        (None, None) => same_ordered_numeric(a, b),
        (Some(_), Some(_)) => false,
    }
}

fn integer_operand_fits(operand: &Expression, value: i128) -> bool {
    match operand.get_type().as_simple() {
        Some(kind) if kind.is_ordered() => match kind.integer_range() {
            Some((min, max)) => min <= value && value <= max,
            None => kind.is_float(),
        },
        _ => false,
    }
}

fn same_ordered_numeric(a: &Expression, b: &Expression) -> bool {
    if is_literal(a) || is_literal(b) {
        return false;
    }
    match (a.get_type().as_simple(), b.get_type().as_simple()) {
        (Some(ka), Some(kb)) => ka == kb && ka.is_ordered(),
        _ => false,
    }
}

fn is_literal(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal { .. } => true,
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => matches!(expression.unwrap_parens(), Expression::Literal { .. }),
        _ => false,
    }
}

pub(crate) fn signed_integer_literal(expression: &Expression) -> Option<i128> {
    if let Some(value) = expression.as_integer() {
        return Some(value as i128);
    }
    if let Expression::Unary {
        operator: UnaryOperator::Negative,
        expression,
        ..
    } = expression
    {
        return expression.as_integer().map(|value| -(value as i128));
    }
    None
}

pub(crate) fn flip_comparison(operator: BinaryOperator) -> BinaryOperator {
    use BinaryOperator::*;
    match operator {
        LessThan => GreaterThan,
        GreaterThan => LessThan,
        LessThanOrEqual => GreaterThanOrEqual,
        GreaterThanOrEqual => LessThanOrEqual,
        other => other,
    }
}

pub(crate) fn is_side_effect_free(expression: &Expression) -> bool {
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

pub(crate) fn expressions_equivalent(a: &Expression, b: &Expression) -> bool {
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

/// The more restrictive of two bounds; `first_wins(a, b)` is true when `a`'s
/// value is tighter. At equal values the bound stays inclusive only if both were.
pub(crate) fn tighter(a: Bound, b: Bound, first_wins: impl Fn(i128, i128) -> bool) -> Bound {
    match (a, b) {
        (None, other) | (other, None) => other,
        (Some((av, ai)), Some((bv, bi))) => {
            if av == bv {
                Some((av, ai && bi))
            } else if first_wins(av, bv) {
                Some((av, ai))
            } else {
                Some((bv, bi))
            }
        }
    }
}
