use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::Type;

use super::helpers::{
    bool_literal, is_one_literal, is_side_effect_free, is_zero_literal, span_text,
};

enum Outcome {
    /// The operation returns its other operand unchanged (`x + 0`, `x && true`).
    Identity,
    /// The operation always evaluates to a constant (`x * 0`, `x && false`).
    Constant(&'static str),
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

pub fn check_redundant_operation(expression: &Expression, ctx: &NodeCtx) {
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

    let unwrapped_left = left.unwrap_parens();
    let unwrapped_right = right.unwrap_parens();

    let Some((side, outcome)) = classify(*operator, unwrapped_left, unwrapped_right) else {
        return;
    };

    let (other, other_source) = match side {
        Side::Left => (unwrapped_left, left.as_ref()),
        Side::Right => (unwrapped_right, right.as_ref()),
    };

    if let Outcome::Constant(value) = outcome {
        if !is_side_effect_free(other) {
            return;
        }
        ctx.sink.push(
            diagnostics::lint::redundant_operation(span, Some(value)).with_fix(Fix::new(
                format!("Replace with `{value}`"),
                Edit::replacement(*span, value),
            )),
        );
    } else {
        let mut diagnostic = diagnostics::lint::redundant_operation(span, None);
        if let Some(text) = span_text(ctx.source, other_source) {
            diagnostic = diagnostic.with_fix(Fix::new(
                format!("Replace with `{text}`"),
                Edit::replacement(*span, text),
            ));
        }
        ctx.sink.push(diagnostic);
    }
}

fn classify(
    operator: BinaryOperator,
    left: &Expression,
    right: &Expression,
) -> Option<(Side, Outcome)> {
    use BinaryOperator::*;

    if let And | Or = operator {
        return classify_boolean(operator, left, right);
    }

    let (side, outcome) = match operator {
        Addition => {
            if is_zero_literal(right) {
                (Side::Left, Outcome::Identity)
            } else if is_zero_literal(left) {
                (Side::Right, Outcome::Identity)
            } else {
                return None;
            }
        }
        Subtraction => {
            if is_zero_literal(right) {
                (Side::Left, Outcome::Identity)
            } else {
                return None;
            }
        }
        Multiplication => {
            if is_one_literal(right) {
                (Side::Left, Outcome::Identity)
            } else if is_one_literal(left) {
                (Side::Right, Outcome::Identity)
            } else if is_zero_literal(right) {
                (Side::Left, Outcome::Constant("0"))
            } else if is_zero_literal(left) {
                (Side::Right, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        Division => {
            if is_one_literal(right) {
                (Side::Left, Outcome::Identity)
            } else {
                return None;
            }
        }
        Remainder => {
            if is_one_literal(right) {
                (Side::Left, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        BitwiseOr | BitwiseXor => {
            if is_zero_literal(right) {
                (Side::Left, Outcome::Identity)
            } else if is_zero_literal(left) {
                (Side::Right, Outcome::Identity)
            } else {
                return None;
            }
        }
        BitwiseAnd => {
            if is_zero_literal(right) {
                (Side::Left, Outcome::Constant("0"))
            } else if is_zero_literal(left) {
                (Side::Right, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        BitwiseAndNot => {
            if is_zero_literal(right) {
                (Side::Left, Outcome::Identity)
            } else if is_zero_literal(left) {
                (Side::Right, Outcome::Constant("0"))
            } else {
                return None;
            }
        }
        ShiftLeft | ShiftRight => {
            // `0 << n` is not folded to `0`: a negative `n` panics at runtime,
            // so the result is not unconditionally `0`.
            if is_zero_literal(right) {
                (Side::Left, Outcome::Identity)
            } else {
                return None;
            }
        }
        _ => return None,
    };

    let other = match side {
        Side::Left => left,
        Side::Right => right,
    };
    if !is_integer(&other.get_type()) {
        return None;
    }
    Some((side, outcome))
}

fn classify_boolean(
    operator: BinaryOperator,
    left: &Expression,
    right: &Expression,
) -> Option<(Side, Outcome)> {
    let is_and = matches!(operator, BinaryOperator::And);
    let (side, outcome) = if let Some(value) = bool_literal(right) {
        (Side::Left, boolean_outcome(value, is_and))
    } else if let Some(value) = bool_literal(left) {
        (Side::Right, boolean_outcome(value, is_and))
    } else {
        return None;
    };
    let other = match side {
        Side::Left => left,
        Side::Right => right,
    };
    if !other.get_type().is_boolean() {
        return None;
    }
    Some((side, outcome))
}

fn boolean_outcome(literal: bool, is_and: bool) -> Outcome {
    if is_and == literal {
        Outcome::Identity
    } else if is_and {
        Outcome::Constant("false")
    } else {
        Outcome::Constant("true")
    }
}

fn is_integer(ty: &Type) -> bool {
    ty.as_simple()
        .is_some_and(|kind| kind.is_signed_int() || kind.is_unsigned_int())
}
