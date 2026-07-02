use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::{has_escaping_control_flow, is_postfix_tight, span_text};

pub fn check_redundant_closure_call(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        raw_type_args,
        span,
        ..
    } = expression
    else {
        return;
    };

    if !args.is_empty() || spread.is_some() || !raw_type_args.is_empty() {
        return;
    }

    let Expression::Lambda {
        params,
        body,
        return_annotation,
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

    let mut diagnostic = diagnostics::lint::redundant_closure_call(span);
    if return_annotation.is_unknown()
        && let Some(inner) = inlinable_value(body)
        && let Some(text) = span_text(ctx.source, inner)
    {
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{text}`"),
            Edit::replacement(*span, text.to_string()),
        ));
    }
    ctx.sink.push(diagnostic);
}

fn inlinable_value(body: &Expression) -> Option<&Expression> {
    let inner = match body.unwrap_parens() {
        Expression::Block { items, .. } => {
            let [single] = items.as_slice() else {
                return None;
            };
            single.unwrap_parens()
        }
        other => other,
    };
    is_postfix_tight(inner).then_some(inner)
}
