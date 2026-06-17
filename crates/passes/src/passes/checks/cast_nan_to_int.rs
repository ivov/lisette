use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;
use syntax::types::SimpleKind;

use super::nan_comparison::is_math_nan_call;

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

    let Some(kind) = ty.underlying_simple_kind() else {
        return;
    };
    if !(kind.is_signed_int() || kind.is_unsigned_int() || kind == SimpleKind::Uintptr) {
        return;
    }

    if !is_math_nan_call(operand.unwrap_parens()) {
        return;
    }

    ctx.sink
        .push(diagnostics::infer::cast_nan_to_int(span, &ty.to_string()));
}
