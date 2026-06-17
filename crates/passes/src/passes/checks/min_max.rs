use crate::passes::comparison::{MinMaxOp, prelude_min_max, signed_integer_literal};
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;
use syntax::types::SimpleKind;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let span = expression.get_span();
    if ctx.claimed_spans.borrow().contains(&span) {
        return;
    }

    let Some(outer) = prelude_min_max(expression) else {
        return;
    };

    // Integers only: a float clamp can yield NaN, which Go's `min`/`max`
    // propagate, so the result is not constant.
    let Some(kind) = expression.get_type().as_simple() else {
        return;
    };
    if kind.integer_range().is_none() {
        return;
    }

    let (outer_constant, nested) = match (
        in_range_literal(outer.left, kind),
        in_range_literal(outer.right, kind),
    ) {
        (Some(constant), None) => (constant, outer.right),
        (None, Some(constant)) => (constant, outer.left),
        _ => return,
    };

    let nested = nested.unwrap_parens();
    let Some(inner) = prelude_min_max(nested) else {
        return;
    };
    if inner.op == outer.op {
        return;
    }

    let inner_constant = match (
        in_range_literal(inner.left, kind),
        in_range_literal(inner.right, kind),
    ) {
        (Some(constant), None) | (None, Some(constant)) => constant,
        _ => return,
    };

    let always_constant = match outer.op {
        MinMaxOp::Min => outer_constant <= inner_constant,
        MinMaxOp::Max => outer_constant >= inner_constant,
    };
    if !always_constant {
        return;
    }

    ctx.claimed_spans.borrow_mut().insert(nested.get_span());
    ctx.sink
        .push(diagnostics::infer::min_max(&span, outer_constant));
}

// In range for `kind`, so the lint never fires on a literal the checker rejects.
fn in_range_literal(expression: &Expression, kind: SimpleKind) -> Option<i128> {
    let value = signed_integer_literal(expression.unwrap_parens())?;
    let (min, max) = kind.integer_range()?;
    (min <= value && value <= max).then_some(value)
}
