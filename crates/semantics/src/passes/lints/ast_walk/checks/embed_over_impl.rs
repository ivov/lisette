use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_embed_over_impl(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Interface { parents, .. } = expression else {
        return;
    };

    for parent in parents {
        if let Some(span) = &parent.impl_keyword_span {
            ctx.sink.push(diagnostics::lint::embed_over_impl(span));
        }
    }
}
