use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::constant_closure_value;

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

    if constant_closure_value(lazy_argument).is_none() {
        return;
    }

    let receiver_ty = receiver.get_type();
    let supported = receiver_ty.is_option()
        || (allows_result && receiver_ty.is_result())
        || (allows_partial && receiver_ty.is_partial());
    if !supported {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::unnecessary_lazy_evaluations(
            &lazy_argument.get_span(),
            lazy,
            eager,
        ));
}
