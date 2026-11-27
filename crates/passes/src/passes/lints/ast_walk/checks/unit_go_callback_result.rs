use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;
use syntax::program::{ValueUseKey, resolved_callee};

pub fn check_unit_go_callback_result(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Lambda { .. } if slot_is_go_function_type(expression, ctx) => {
            check_lambda(expression, ctx);
        }
        Expression::Call {
            expression: callee,
            args,
            ..
        } if resolved_callee(callee).is_some_and(|callee| callee.is_go()) => {
            for argument in args {
                let argument = argument.unwrap_parens();
                if !slot_is_go_function_type(argument, ctx) {
                    check_lambda(argument, ctx);
                }
            }
        }
        _ => {}
    }
}

fn slot_is_go_function_type(expression: &Expression, ctx: &NodeCtx) -> bool {
    ctx.facts
        .lambdas_in_go_function_slots
        .contains(&ValueUseKey::value(expression).expression())
}

fn check_lambda(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Lambda { body, ty, span, .. } = expression else {
        return;
    };
    if !ctx.store.resolves_to_unit(&body.get_type()) {
        return;
    }
    let Some(result) = ty.get_function_ret() else {
        return;
    };
    let is_interface = ctx.store.resolves_to_unknown(result)
        || ctx.store.is_interface(&ctx.store.peel_alias(result));
    if !is_interface {
        return;
    }
    ctx.sink
        .push(diagnostics::lint::unit_lambda_into_go_interface_result(
            span,
            &result.to_string(),
        ));
}
