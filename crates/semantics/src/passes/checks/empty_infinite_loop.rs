use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::Loop { body, span, .. } = expression
        && let Expression::Block { items, .. } = body.as_ref()
        && items.is_empty()
    {
        ctx.sink.push(diagnostics::infer::empty_infinite_loop(span));
    }
}
