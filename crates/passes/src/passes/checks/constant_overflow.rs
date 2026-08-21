use crate::passes::comparison::signed_integer_literal;
use crate::passes::walk::{ClaimKind, NodeCtx};
use syntax::ast::{Constant, Expression};

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
    let span = expression.get_span();

    if ctx.is_claimed(ClaimKind::ConstantOverflow, &span) {
        return;
    }

    let Some(constant) = expression.fold_constant() else {
        return;
    };

    // Go checks the range of the whole expression, so `(max + 1) - 1` is legal.
    claim_operands(expression, ctx);

    // The checker reports a lone literal as integer_literal_overflow.
    if signed_integer_literal(expression).is_some() {
        return;
    }

    let ty = expression.get_type();
    let Some(kind) = ctx.store.underlying_simple_kind(&ty) else {
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

    ctx.sink.push(diagnostics::infer::constant_overflow(
        &span,
        &ty.to_string(),
        value,
        min,
        max,
    ));
}

/// Marks a constant subtree, so no part of it is range checked on its own.
pub(super) fn claim_constant(expression: &Expression, ctx: &mut NodeCtx) {
    ctx.claim(ClaimKind::ConstantOverflow, expression.get_span());
    claim_operands(expression, ctx);
}

fn claim_operands(expression: &Expression, ctx: &mut NodeCtx) {
    for child in expression.children() {
        ctx.claim(ClaimKind::ConstantOverflow, child.get_span());
        claim_operands(child, ctx);
    }
}
