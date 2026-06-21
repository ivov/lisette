use crate::passes::lints::ast_walk::casing::to_snake_case;
use crate::passes::walk::NodeCtx;
use syntax::ast::{Annotation, Expression};

use super::helpers::first_param_is_self;

pub fn check_self_named_constructors(expression: &Expression, ctx: &NodeCtx) {
    if ctx.is_d_lis {
        return;
    }

    let Expression::ImplBlock {
        receiver_name,
        methods,
        ..
    } = expression
    else {
        return;
    };

    if receiver_name.is_empty() {
        return;
    }

    let expected = to_snake_case(receiver_name);

    for method in methods {
        let Expression::Function {
            name,
            name_span,
            params,
            return_annotation,
            ..
        } = method
        else {
            continue;
        };

        if first_param_is_self(params) {
            continue;
        }

        if name.as_str() != expected {
            continue;
        }

        let returns_self = matches!(
            return_annotation,
            Annotation::Constructor { name: returned, .. } if returned == receiver_name
        );
        if !returns_self {
            continue;
        }

        ctx.sink.push(diagnostics::lint::self_named_constructors(
            name_span,
            receiver_name,
            name,
        ));
    }
}
