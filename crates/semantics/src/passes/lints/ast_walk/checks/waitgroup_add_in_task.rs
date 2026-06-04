use crate::passes::walk::NodeCtx;
use rustc_hash::FxHashSet;
use syntax::ast::{BindingId, Expression, Literal, Span, UnaryOperator};
use syntax::types::Type;

pub fn check_waitgroup_add_in_task(expression: &Expression, ctx: &NodeCtx) {
    let body = match expression {
        Expression::Function { body, .. } | Expression::Lambda { body, .. } => body,
        _ => return,
    };

    let mut waited: FxHashSet<BindingId> = FxHashSet::default();
    let mut adds: Vec<(BindingId, Span)> = Vec::new();
    collect(body, false, &mut waited, &mut adds);

    for (binding, span) in adds {
        if waited.contains(&binding) {
            ctx.sink
                .push(diagnostics::lint::waitgroup_add_in_task(&span));
        }
    }
}

/// Gather every `WaitGroup` `Wait` reached outside a `task`, and every positive
/// `Add` reached inside one. Nested functions and lambdas are their own roots,
/// so descent stops at their boundary.
fn collect(
    expression: &Expression,
    in_task: bool,
    waited: &mut FxHashSet<BindingId>,
    adds: &mut Vec<(BindingId, Span)>,
) {
    match expression {
        Expression::Function { .. } | Expression::Lambda { .. } => return,
        Expression::Task { expression, .. } => {
            collect(expression, true, waited, adds);
            return;
        }
        Expression::Call {
            expression: callee,
            args,
            span,
            ..
        } => {
            if let Some((member, binding)) = waitgroup_method(callee) {
                match member {
                    "Wait" if !in_task => {
                        waited.insert(binding);
                    }
                    "Add" if in_task => {
                        if args
                            .first()
                            .is_some_and(|delta| !is_nonpositive_literal(delta))
                        {
                            adds.push((binding, *span));
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    for child in expression.children() {
        collect(child, in_task, waited, adds);
    }
}

fn waitgroup_method(callee: &Expression) -> Option<(&str, BindingId)> {
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    let Expression::Identifier {
        binding_id: Some(binding),
        ..
    } = receiver.unwrap_parens()
    else {
        return None;
    };
    if !is_waitgroup(&receiver.get_type()) {
        return None;
    }
    Some((member.as_str(), *binding))
}

fn is_waitgroup(ty: &Type) -> bool {
    ty.strip_refs().get_qualified_id() == Some("go:sync.WaitGroup")
}

/// A zero or negative delta is the `Done` equivalent and is legitimate inside a
/// `task`; only a positive (or unknown) delta must precede `Wait`.
fn is_nonpositive_literal(delta: &Expression) -> bool {
    match delta.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Integer { value, .. },
            ..
        } => *value == 0,
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => matches!(
            expression.unwrap_parens(),
            Expression::Literal { literal: Literal::Integer { value, .. }, .. } if *value != 0
        ),
        _ => false,
    }
}
