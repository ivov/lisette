use super::helpers::reaches_loop_jump;
use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal};

pub fn check_single_element_loop(expression: &Expression, ctx: &NodeCtx) {
    let Expression::For { iterable, body, .. } = expression else {
        return;
    };

    let Expression::Literal {
        literal: Literal::Slice(elements),
        span: iterable_span,
        ..
    } = iterable.unwrap_parens()
    else {
        return;
    };
    if elements.len() != 1 {
        return;
    }

    // A `break`/`continue` would be invalid once the loop is gone.
    if reaches_loop_jump(body, true) {
        return;
    }

    // A body that always exits is `loop_runs_once`, which owns it.
    if body.diverges().is_some() {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::single_element_loop(iterable_span));
}
