use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

use super::helpers::{expressions_equivalent, is_side_effect_free};

/// (module_id, function_name, arg index a, arg index b)
const DUP_ARG_TARGETS: &[(&str, &str, usize, usize)] = &[
    ("go:math", "Max", 0, 1),
    ("go:math", "Min", 0, 1),
    ("go:reflect", "DeepEqual", 0, 1),
    ("go:bytes", "Equal", 0, 1),
    ("go:bytes", "Compare", 0, 1),
    ("go:bytes", "EqualFold", 0, 1),
    ("go:strings", "Compare", 0, 1),
    ("go:strings", "EqualFold", 0, 1),
    ("go:strings", "Replace", 1, 2),
    ("go:strings", "ReplaceAll", 1, 2),
];

pub fn check_dup_arg(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        span,
        ..
    } = expression
    else {
        return;
    };

    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    let namespace_ty = namespace.get_type();
    let Some(module_id) = namespace_ty.as_import_namespace() else {
        return;
    };

    for (target_module, target_function, a, b) in DUP_ARG_TARGETS {
        if module_id != *target_module || member != *target_function {
            continue;
        }
        let (Some(arg_a), Some(arg_b)) = (args.get(*a), args.get(*b)) else {
            return;
        };
        if !is_side_effect_free(arg_a) || !is_side_effect_free(arg_b) {
            return;
        }
        if !expressions_equivalent(arg_a, arg_b) {
            return;
        }
        ctx.sink.push(diagnostics::lint::duplicate_arguments(
            span,
            target_module,
            target_function,
        ));
        return;
    }
}
