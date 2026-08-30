use crate::passes::walk::NodeCtx;
use rustc_hash::FxHashSet;
use std::slice;
use syntax::ast::{BindingId, Expression, IdentifierResolution, Literal, Span, UnaryOperator};
use syntax::types::Type;

pub fn check_waitgroup(expression: &Expression, ctx: &NodeCtx) {
    let body = match expression {
        Expression::Function { body, .. } => {
            let Some(body) = body.definition() else {
                return;
            };
            body
        }
        Expression::Lambda { body, .. } => body.as_ref(),
        _ => return,
    };

    let mut waited: FxHashSet<BindingId> = FxHashSet::default();
    let mut covered: FxHashSet<BindingId> = FxHashSet::default();
    let mut adds: Vec<(BindingId, Span)> = Vec::new();
    collect(body, false, &mut waited, &mut covered, &mut adds);

    let mut warned: FxHashSet<Span> = FxHashSet::default();
    for (binding, span) in adds {
        if waited.contains(&binding) && !covered.contains(&binding) {
            ctx.sink
                .push(diagnostics::lint::waitgroup_add_in_task(&span));
            warned.insert(span);
        }
    }

    collect_manual_go(body, &warned, ctx);
}

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

/// `waitgroup_add_in_task` wins a span it already took, so `warned` holds those.
fn collect_manual_go(expression: &Expression, warned: &FxHashSet<Span>, ctx: &NodeCtx) {
    match expression {
        Expression::Function { .. } | Expression::Lambda { .. } => return,
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => {
            for pair in items.windows(2) {
                let Some((binding, span)) = unit_add(&pair[0]) else {
                    continue;
                };
                let Expression::Task {
                    expression: task_body,
                    ..
                } = &pair[1]
                else {
                    continue;
                };
                if !warned.contains(&span) && task_only_signals_done(task_body, binding) {
                    ctx.sink.push(diagnostics::lint::manual_waitgroup_go(&span));
                }
            }
        }
        _ => {}
    }

    for child in expression.children() {
        collect_manual_go(child, warned, ctx);
    }
}

fn unit_add(expression: &Expression) -> Option<(BindingId, Span)> {
    let Expression::Call {
        expression: callee,
        args,
        spread: None,
        span,
        ..
    } = expression
    else {
        return None;
    };
    let ("Add", binding) = waitgroup_method(callee)? else {
        return None;
    };
    let [delta] = args.as_slice() else {
        return None;
    };
    matches!(
        delta.unwrap_parens(),
        Expression::Literal { literal: Literal::Integer { value, .. }, .. } if *value == 1
    )
    .then_some((binding, *span))
}

fn task_only_signals_done(task_body: &Expression, binding: BindingId) -> bool {
    let items: &[Expression] = match task_body {
        Expression::Block { items, .. } => items,
        other => slice::from_ref(other),
    };

    let mut done: Option<(usize, bool)> = None;
    for (index, item) in items.iter().enumerate() {
        let (call, deferred) = match item {
            Expression::Defer { expression, .. } => (expression.as_ref(), true),
            other => (other, false),
        };
        if !is_done_call(call, binding) {
            continue;
        }
        if done.is_some() {
            return false;
        }
        done = Some((index, deferred));
    }

    let Some((index, deferred)) = done else {
        return false;
    };
    if items[..index].iter().any(has_done_order_blocker) {
        return false;
    }
    if !deferred && index + 1 != items.len() {
        return false;
    }
    mentions(task_body, binding) == 1
}

fn is_done_call(expression: &Expression, binding: BindingId) -> bool {
    let Expression::Call {
        expression: callee,
        args,
        spread: None,
        ..
    } = expression
    else {
        return false;
    };
    args.is_empty() && waitgroup_method(callee) == Some(("Done", binding))
}

/// An exit skips a later `Done`, and a `defer` runs after it under Go's last in
/// first out order, while `Go` runs `Done` last on every exit.
fn has_done_order_blocker(expression: &Expression) -> bool {
    match expression {
        Expression::Function { .. } | Expression::Lambda { .. } | Expression::Task { .. } => {
            return false;
        }
        Expression::Return { .. }
        | Expression::Break { .. }
        | Expression::Continue { .. }
        | Expression::Propagate { .. }
        | Expression::Defer { .. } => return true,
        _ => {}
    }

    expression
        .children()
        .into_iter()
        .any(has_done_order_blocker)
}

fn mentions(expression: &Expression, binding: BindingId) -> usize {
    let own = usize::from(matches!(
        expression,
        Expression::Identifier {
            resolution: IdentifierResolution::Binding(id),
            ..
        } if *id == binding
    ));

    own + expression
        .children()
        .into_iter()
        .map(|child| mentions(child, binding))
        .sum::<usize>()
}

pub(super) fn waitgroup_method(callee: &Expression) -> Option<(&str, BindingId)> {
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    let Expression::Identifier {
        resolution: IdentifierResolution::Binding(binding),
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
/// `task`. Only a positive (or unknown) delta must precede `Wait`.
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
