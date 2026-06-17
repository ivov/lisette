use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_self_assignment(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Assignment {
        target,
        value,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (
        Expression::Identifier {
            value: target_name, ..
        },
        Expression::Identifier {
            value: value_name, ..
        },
    ) = (target.unwrap_parens(), value.unwrap_parens())
    else {
        return;
    };

    if target_name != value_name {
        return;
    }

    ctx.sink.push(diagnostics::lint::self_assignment(span));
}
