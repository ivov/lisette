use crate::passes::comparison::signed_integer_literal;
use crate::passes::walk::NodeCtx;
use syntax::ast::{Constant, Expression};
use syntax::types::{SimpleKind, Type};

use super::constant_overflow::claim_constant;

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
    let Expression::Cast {
        expression: operand,
        ty,
        span,
        ..
    } = expression
    else {
        return;
    };

    let Some(constant) = operand.fold_constant() else {
        return;
    };

    // The cast gives the constant its type, so the operand needs no check.
    claim_constant(operand, ctx);

    let Some(kind) = ctx.store.underlying_simple_kind(ty) else {
        return;
    };
    let Some((min, max)) = kind.integer_range() else {
        return;
    };

    let Constant::Integer(value) = constant else {
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

    if ctx.store.has_underlying_rune(&operand.get_type()) && ctx.store.has_underlying_byte(ty) {
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
