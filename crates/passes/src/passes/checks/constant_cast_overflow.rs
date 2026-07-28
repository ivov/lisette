use crate::passes::comparison::signed_integer_literal;
use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::types::{SimpleKind, Type};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Cast {
        expression: operand,
        ty,
        span,
        ..
    } = expression
    else {
        return;
    };

    let Some(value) = fold(operand) else {
        return;
    };

    let Some(kind) = ctx.store.underlying_simple_kind(ty) else {
        return;
    };
    let Some((min, max)) = kind.integer_range() else {
        return;
    };

    if (min..=max).contains(&value) {
        return;
    }

    // The checker reports a lone literal as integer_literal_overflow, but only
    // for primitive target names, leaving alias and newtype targets to us.
    if names_primitive_integer(ty) && signed_integer_literal(operand.unwrap_parens()).is_some() {
        return;
    }

    ctx.sink.push(diagnostics::infer::constant_cast_overflow(
        span,
        &ty.to_string(),
        value,
        min,
        max,
    ));
}

fn names_primitive_integer(ty: &Type) -> bool {
    ty.get_name()
        .and_then(SimpleKind::from_name)
        .and_then(SimpleKind::integer_range)
        .is_some()
}

/// Literals only, because a named constant carries a Go type whose arithmetic
/// and `^` mask differ from the untyped semantics `i128` models here.
fn fold(expression: &Expression) -> Option<i128> {
    match expression.unwrap_parens() {
        // Negative literal text occurs only in patterns, so bail rather than
        // misread the magnitude.
        Expression::Literal {
            literal: Literal::Integer { value, text },
            ..
        } if !text.as_deref().is_some_and(|text| text.starts_with('-')) => Some(*value as i128),
        Expression::Unary {
            operator,
            expression,
            ..
        } => {
            let value = fold(expression)?;
            match operator {
                UnaryOperator::Negative => value.checked_neg(),
                UnaryOperator::BitwiseNot => Some(!value),
                _ => None,
            }
        }
        Expression::Binary {
            operator,
            left,
            right,
            ..
        } => fold_binary(*operator, fold(left)?, fold(right)?),
        _ => None,
    }
}

fn fold_binary(operator: BinaryOperator, left: i128, right: i128) -> Option<i128> {
    match operator {
        BinaryOperator::Addition => left.checked_add(right),
        BinaryOperator::Subtraction => left.checked_sub(right),
        BinaryOperator::Multiplication => left.checked_mul(right),
        BinaryOperator::Division => left.checked_div(right),
        BinaryOperator::Remainder => left.checked_rem(right),
        BinaryOperator::BitwiseAnd => Some(left & right),
        BinaryOperator::BitwiseOr => Some(left | right),
        BinaryOperator::BitwiseXor => Some(left ^ right),
        BinaryOperator::BitwiseAndNot => Some(left & !right),
        BinaryOperator::ShiftLeft => {
            let amount = shift_amount(right)?;
            let shifted = left << amount;
            (shifted >> amount == left).then_some(shifted)
        }
        BinaryOperator::ShiftRight => Some(left >> shift_amount(right)?),
        _ => None,
    }
}

fn shift_amount(amount: i128) -> Option<u32> {
    (0..128).contains(&amount).then_some(amount as u32)
}
