use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::{as_tight_operand, constant_closure_value, expression_is_pure, span_text};

pub fn check_unnecessary_lazy_evaluations(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return;
    };
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    let lazy = member.as_str();
    let (eager, lazy_argument, allows_result, allows_partial) = match lazy {
        "unwrap_or_else" => {
            let [closure] = args.as_slice() else {
                return;
            };
            ("unwrap_or", closure, true, true)
        }
        "ok_or_else" => {
            let [closure] = args.as_slice() else {
                return;
            };
            ("ok_or", closure, false, false)
        }
        "map_or_else" => {
            let [default, _mapper] = args.as_slice() else {
                return;
            };
            ("map_or", default, true, false)
        }
        _ => return,
    };

    let Some(constant) = constant_closure_value(lazy_argument) else {
        return;
    };

    if !expression_is_pure(constant, ctx.store) {
        return;
    }

    let receiver_ty = receiver.get_type();
    let supported = receiver_ty.is_option()
        || (allows_result && receiver_ty.is_result())
        || (allows_partial && receiver_ty.is_partial());
    if !supported {
        return;
    }

    let mut diagnostic =
        diagnostics::lint::unnecessary_lazy_evaluations(&lazy_argument.get_span(), lazy, eager);
    let rest_texts: Option<Vec<&str>> = args[1..]
        .iter()
        .map(|arg| span_text(ctx.source, arg))
        .collect();
    if let (Some(receiver_text), Some(constant_text), Some(rest_texts)) = (
        span_text(ctx.source, receiver),
        span_text(ctx.source, constant),
        rest_texts,
    ) {
        let mut call_args = constant_text.to_string();
        for extra in rest_texts {
            call_args.push_str(", ");
            call_args.push_str(extra);
        }
        let replacement = format!(
            "{}.{eager}({call_args})",
            as_tight_operand(receiver_text, receiver)
        );
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(expression.get_span(), replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}
