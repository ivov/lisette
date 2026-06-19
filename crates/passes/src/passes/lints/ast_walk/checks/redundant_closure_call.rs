use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::has_escaping_control_flow;

pub fn check_redundant_closure_call(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        type_args,
        span,
        ..
    } = expression
    else {
        return;
    };

    if !args.is_empty() || spread.is_some() || !type_args.is_empty() {
        return;
    }

    let Expression::Lambda {
        params,
        body,
        span: lambda_span,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };
    if !params.is_empty() {
        return;
    }

    // A `return`/`?`/`break`/`continue` in the body targets the closure; inlining
    // would retarget the jump to the enclosing scope.
    if has_escaping_control_flow(body) {
        return;
    }

    // Claim the closure so `redundant_closure` does not also advise on it.
    ctx.claimed_spans.borrow_mut().insert(*lambda_span);

    ctx.sink
        .push(diagnostics::lint::redundant_closure_call(span));
}
