use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::{is_zero_literal, method_call};

pub fn check_unnecessary_first_then_check(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        span,
        ..
    } = expression
    else {
        return;
    };
    if !args.is_empty() {
        return;
    }
    let Expression::DotAccess {
        expression: get_call,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };
    let negate = match member.as_str() {
        "is_some" => true,
        "is_none" => false,
        _ => return,
    };

    let Some((slice, get_args, _)) = method_call(get_call.unwrap_parens(), "get") else {
        return;
    };
    let [index] = get_args else {
        return;
    };
    if !is_zero_literal(index.unwrap_parens()) {
        return;
    }
    if !slice.get_type().is_slice() {
        return;
    }

    let Some(slice_text) = slice.as_dotted_path() else {
        return;
    };
    let replacement = if negate {
        format!("!{slice_text}.is_empty()")
    } else {
        format!("{slice_text}.is_empty()")
    };

    ctx.sink
        .push(diagnostics::lint::unnecessary_first_then_check(
            span,
            &replacement,
        ));
}
