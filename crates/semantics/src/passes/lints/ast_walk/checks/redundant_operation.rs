use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::types::Type;

use super::helpers::{bool_literal, is_side_effect_free};

enum Outcome {
    /// The operation returns its other operand unchanged (`x + 0`, `x && true`).
    Identity,
    /// The operation always evaluates to a constant (`x * 0`, `x && false`).
    Constant(&'static str),
}

pub fn check_redundant_operation(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();

    let Some((other, outcome)) = classify(*operator, left, right) else {
        return;
    };

    if let Outcome::Constant(value) = outcome {
        if !is_side_effect_free(other) {
            return;
        }
        diagnostics.push(diagnostics::lint::redundant_operation(span, Some(value)));
    } else {
        diagnostics.push(diagnostics::lint::redundant_operation(span, None));
    }
}

fn classify<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, Outcome)> {
    use BinaryOperator::*;

    if let And | Or = operator {
        return classify_boolean(operator, left, right);
    }

    let (other, outcome) = match operator {
        Addition => {
            if int_zero(right) {
                (left, Outcome::Identity)
            } else if int_zero(left) {
                (right, Outcome::Identity)
            } else {
                return None;
            }
        }
        Subtraction => {
            if int_zero(right) {
                (left, Outcome::Identity)
            } else {
                return None;
            }
        }
        Multiplication => {
            if int_one(right) {
                (left, Outcome::Identity)
            } else if int_one(left) {
                (right, Outcome::Identity)
            } else if int_zero(right) {
                (left, Outcome::Constant("0"))
            } else if int_zero(left) {
                (right, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        Division => {
            if int_one(right) {
                (left, Outcome::Identity)
            } else {
                return None;
            }
        }
        Remainder => {
            if int_one(right) {
                (left, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        BitwiseOr | BitwiseXor => {
            if int_zero(right) {
                (left, Outcome::Identity)
            } else if int_zero(left) {
                (right, Outcome::Identity)
            } else {
                return None;
            }
        }
        BitwiseAnd => {
            if int_zero(right) {
                (left, Outcome::Constant("0"))
            } else if int_zero(left) {
                (right, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        BitwiseAndNot => {
            if int_zero(right) {
                (left, Outcome::Identity)
            } else if int_zero(left) {
                (right, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        ShiftLeft | ShiftRight => {
            // `0 << n` is not folded to `0`: a negative `n` panics at runtime,
            // so the result is not unconditionally `0`.
            if int_zero(right) {
                (left, Outcome::Identity)
            } else {
                return None;
            }
        }
        _ => return None,
    };

    if !is_integer(&other.get_type()) {
        return None;
    }
    Some((other, outcome))
}

fn classify_boolean<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, Outcome)> {
    let is_and = matches!(operator, BinaryOperator::And);
    let (other, outcome) = if let Some(value) = bool_literal(right) {
        boolean_outcome(left, value, is_and)
    } else if let Some(value) = bool_literal(left) {
        boolean_outcome(right, value, is_and)
    } else {
        return None;
    };
    if !other.get_type().is_boolean() {
        return None;
    }
    Some((other, outcome))
}

fn boolean_outcome(other: &Expression, literal: bool, is_and: bool) -> (&Expression, Outcome) {
    if is_and == literal {
        (other, Outcome::Identity)
    } else if is_and {
        (other, Outcome::Constant("false"))
    } else {
        (other, Outcome::Constant("true"))
    }
}

fn is_integer(ty: &Type) -> bool {
    ty.as_simple()
        .is_some_and(|kind| kind.is_signed_int() || kind.is_unsigned_int())
}

fn int_zero(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 0, .. },
            ..
        }
    )
}

fn int_one(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 1, .. },
            ..
        }
    )
}
