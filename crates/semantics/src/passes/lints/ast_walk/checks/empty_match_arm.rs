use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_empty_match_arm(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match { arms, .. } = expression else {
        return;
    };

    for arm in arms {
        if let Expression::Block { items, span, .. } = &*arm.expression
            && items.is_empty()
        {
            ctx.sink.push(diagnostics::lint::empty_match_arm(span));
        }
    }
}
