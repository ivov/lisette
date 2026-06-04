use syntax::ast::Expression;

use crate::passes::lints::ast_walk::casing::{is_screaming_snake_case, to_screaming_snake_case};
use crate::passes::walk::NodeCtx;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if ctx.is_d_lis {
        return;
    }
    let Expression::Const {
        identifier,
        identifier_span,
        ..
    } = expression
    else {
        return;
    };
    if identifier.starts_with('_') || is_screaming_snake_case(identifier) {
        return;
    }
    ctx.sink.push(diagnostics::lint::miscased_screaming_snake(
        identifier_span,
        &to_screaming_snake_case(identifier),
    ));
}
