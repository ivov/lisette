use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, IdentifierResolution, Pattern};
use syntax::program::{CallKind, DotAccessKind};
use syntax::types::Type;

use super::helpers::lambda_is_annotated;
use crate::passes::walk::NodeCtx;
use semantics::facts::Facts;

pub fn check_redundant_closure(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Lambda {
        params,
        body,
        span,
        ty: lambda_ty,
        ..
    } = expression
    else {
        return;
    };

    // An immediately-invoked closure is owned by `redundant_closure_call`, which
    // claims this span when it removes the wrapper.
    if ctx.claimed_spans.borrow().contains(span) {
        return;
    }

    let Expression::Call {
        expression: callee,
        args,
        spread,
        type_arguments,
        call_kind,
        ..
    } = lambda_body(body)
    else {
        return;
    };

    if !matches!(call_kind, CallKind::Regular)
        || spread.is_some()
        || !type_arguments.is_empty()
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

    let callee = callee.unwrap_parens();
    let callee_ty = callee.get_type();

    if !signatures_match(lambda_ty, &callee_ty) {
        return;
    }

    let Some(callee_name) = hoistable_callee(callee, &callee_ty, &param_names, ctx.facts) else {
        return;
    };

    let mut diagnostic = diagnostics::lint::redundant_closure(span, &callee_name);
    if !lambda_is_annotated(expression) {
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{callee_name}`"),
            Edit::replacement(*span, callee_name.clone()),
        ));
    }
    ctx.sink.push(diagnostic);
}

fn signatures_match(lambda_ty: &Type, callee_ty: &Type) -> bool {
    let (lambda_positions, callee_positions) = (
        lambda_ty.unwrap_forall().children(),
        callee_ty.unwrap_forall().children(),
    );

    lambda_positions.len() == callee_positions.len()
        && lambda_positions
            .iter()
            .zip(&callee_positions)
            .all(|(closure_ty, callee_ty)| {
                closure_ty == callee_ty || is_unresolved(closure_ty) || is_unresolved(callee_ty)
            })
}

fn is_unresolved(ty: &Type) -> bool {
    matches!(ty, Type::Var { .. } | Type::Parameter(_))
}

fn lambda_body(body: &Expression) -> &Expression {
    match body.unwrap_parens() {
        Expression::Block { items, .. } if items.len() == 1 => items[0].unwrap_parens(),
        other => other,
    }
}

fn hoistable_callee(
    callee: &Expression,
    callee_ty: &Type,
    params: &[&str],
    facts: &Facts,
) -> Option<String> {
    // A `mut`-param callee (e.g. `sort.Ints`) is valid only wrapped in a closure,
    // never as a bare function value.
    if callee_ty
        .get_function_params()
        .is_some_and(|params| params.iter().any(|param| param.mutable))
    {
        return None;
    }
    match callee {
        Expression::Identifier {
            value, resolution, ..
        } => {
            if params.contains(&value.as_str()) {
                return None;
            }
            // A reassignable capture is read lazily by the closure but bound
            // eagerly as a bare reference, so hoisting it would change behavior.
            if let IdentifierResolution::Binding(id) = resolution {
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
            resolution,
            ..
        } if resolution.kind() == Some(DotAccessKind::ModuleMember) => {
            let Expression::Identifier { value: base, .. } = base.unwrap_parens() else {
                return None;
            };
            Some(format!("{base}.{member}"))
        }
        _ => None,
    }
}
