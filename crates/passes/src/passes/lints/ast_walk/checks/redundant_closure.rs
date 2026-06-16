use syntax::ast::{Expression, Pattern};
use syntax::program::{CallKind, DotAccessKind};

use crate::passes::walk::NodeCtx;
use semantics::facts::Facts;

pub fn check_redundant_closure(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Lambda {
        params, body, span, ..
    } = expression
    else {
        return;
    };

    let Expression::Call {
        expression: callee,
        args,
        spread,
        type_args,
        call_kind,
        ..
    } = lambda_body(body)
    else {
        return;
    };

    if !matches!(call_kind, Some(CallKind::Regular))
        || spread.is_some()
        || !type_args.is_empty()
        || args.len() != params.len()
    {
        return;
    }

    let mut param_names = Vec::with_capacity(params.len());
    for (param, arg) in params.iter().zip(args) {
        let Pattern::Identifier { identifier, .. } = &param.pattern else {
            return;
        };
        let Expression::Identifier { value, .. } = arg.unwrap_parens() else {
            return;
        };
        if identifier.as_str() != value.as_str() {
            return;
        }
        param_names.push(identifier.as_str());
    }

    let Some(callee_name) = hoistable_callee(callee.unwrap_parens(), &param_names, ctx.facts)
    else {
        return;
    };

    ctx.sink
        .push(diagnostics::lint::redundant_closure(span, &callee_name));
}

fn lambda_body(body: &Expression) -> &Expression {
    match body.unwrap_parens() {
        Expression::Block { items, .. } if items.len() == 1 => items[0].unwrap_parens(),
        other => other,
    }
}

fn hoistable_callee(callee: &Expression, params: &[&str], facts: &Facts) -> Option<String> {
    // A `mut`-param callee (e.g. `sort.Ints`) is valid only wrapped in a closure,
    // never as a bare function value.
    if callee.get_type().get_param_mutability().iter().any(|m| *m) {
        return None;
    }
    match callee {
        Expression::Identifier {
            value, binding_id, ..
        } => {
            if params.contains(&value.as_str()) {
                return None;
            }
            // A reassignable capture is read lazily by the closure but bound
            // eagerly as a bare reference, so hoisting it would change behavior.
            if let Some(id) = binding_id {
                match facts.bindings.get(id) {
                    Some(binding) if !binding.kind.is_mutable() => {}
                    _ => return None,
                }
            }
            Some(value.to_string())
        }
        Expression::DotAccess {
            expression: base,
            member,
            dot_access_kind: Some(DotAccessKind::ModuleMember),
            ..
        } => {
            let Expression::Identifier { value: base, .. } = base.unwrap_parens() else {
                return None;
            };
            Some(format!("{base}.{member}"))
        }
        _ => None,
    }
}
