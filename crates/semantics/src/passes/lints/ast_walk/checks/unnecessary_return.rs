use crate::passes::walk::NodeCtx;
use diagnostics::LocalSink;
use syntax::ast::Expression;

pub fn check_unnecessary_return(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Function { body, .. } = expression else {
        return;
    };

    flag_tail_returns(body, ctx.sink);
}

/// Flag a `return <value>` sitting in tail position, where the value is what
/// the surrounding block yields anyway.
fn flag_tail_returns(expression: &Expression, sink: &LocalSink) {
    match expression {
        Expression::Return {
            expression: value,
            span,
            ..
        } => {
            // Bare `return` is excluded: dropping it can change the block's
            // type when a preceding statement is non-unit.
            if !matches!(value.as_ref(), Expression::Unit { .. }) {
                sink.push(diagnostics::lint::unnecessary_return(span));
            }
        }
        Expression::Block { items, .. } => {
            if let Some(last) = items.last() {
                flag_tail_returns(last, sink);
            }
        }
        Expression::If {
            consequence,
            alternative,
            ..
        }
        | Expression::IfLet {
            consequence,
            alternative,
            ..
        } => {
            flag_tail_returns(consequence, sink);
            flag_tail_returns(alternative, sink);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                flag_tail_returns(&arm.expression, sink);
            }
        }
        _ => {}
    }
}
