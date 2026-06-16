use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::wrapped_single_arg;

pub fn check_needless_question_mark(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Function { body, .. } => flag_tail(body, ctx),
        Expression::Return {
            expression: value, ..
        } => flag_needless(value, ctx),
        _ => {}
    }
}

// Tail position propagates through block tails, both branches of an `if`, and
// every `match` arm. Explicit `return` statements are caught on their own node
// visit instead.
fn flag_tail(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Block { items, .. } => {
            if let Some(last) = items.last() {
                flag_tail(last, ctx);
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
            flag_tail(consequence, ctx);
            flag_tail(alternative, ctx);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                flag_tail(&arm.expression, ctx);
            }
        }
        other => flag_needless(other, ctx),
    }
}

// In return or tail position, `Some(x?)` and `Ok(x?)` propagate then re-wrap the
// same container, so the function returns `x` unchanged.
fn flag_needless(value: &Expression, ctx: &NodeCtx) {
    let value = value.unwrap_parens();
    let (wrapper, arg) = if let Some(arg) = wrapped_single_arg(value, "Some") {
        ("Some", arg)
    } else if let Some(arg) = wrapped_single_arg(value, "Ok") {
        ("Ok", arg)
    } else {
        return;
    };

    let Expression::Propagate {
        expression: inner, ..
    } = arg.unwrap_parens()
    else {
        return;
    };

    let wrapped = value.get_type();
    let confirmed = match wrapper {
        "Some" => wrapped.is_option(),
        _ => wrapped.is_result(),
    };
    if !confirmed {
        return;
    }

    // Equal types rule out an error-type conversion on `?` that `x` alone would
    // not reproduce, and rule out firing on unresolved (mismatched) operands.
    if wrapped != inner.get_type() {
        return;
    }

    ctx.sink.push(diagnostics::lint::needless_question_mark(
        &value.get_span(),
        wrapper,
    ));
}
