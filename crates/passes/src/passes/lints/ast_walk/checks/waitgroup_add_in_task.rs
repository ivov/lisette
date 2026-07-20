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
    let mut covered: FxHashSet<BindingId> = FxHashSet::default();
    let mut adds: Vec<(BindingId, Span)> = Vec::new();
    collect(body, false, &mut waited, &mut covered, &mut adds);

    for (binding, span) in adds {
        if waited.contains(&binding) && !covered.contains(&binding) {
            ctx.sink
                .push(diagnostics::lint::waitgroup_add_in_task(&span));
        }
    }
}

/// A positive `Add` outside a `task` happens-before any task it spawns, so it
/// covers `Add`s inside them. Nested functions/lambdas are their own roots.
fn collect(
    expression: &Expression,
    in_task: bool,
    waited: &mut FxHashSet<BindingId>,
    covered: &mut FxHashSet<BindingId>,
    adds: &mut Vec<(BindingId, Span)>,
) {
    match expression {
        Expression::Function { .. } | Expression::Lambda { .. } => return,
        Expression::Task { expression, .. } => {
            collect(expression, true, waited, covered, adds);
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
                    "Add" => {
                        let positive = args
                            .first()
                            .is_some_and(|delta| !is_nonpositive_literal(delta));
                        if positive {
                            if in_task {
                                adds.push((binding, *span));
                            } else {
                                covered.insert(binding);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    for child in expression.children() {
        collect(child, in_task, waited, covered, adds);
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
