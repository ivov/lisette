use crate::passes::walk::{FunctionRole, NodeCtx};
use syntax::ast::{BinaryOperator, Expression, LetMode, MatchArm, Span};
use syntax::program::resolved_definition;
use syntax::types::{Symbol, Type};

pub fn check_unconditional_recursion(expression: &Expression, ctx: &NodeCtx, role: FunctionRole) {
    let Expression::Function { name, body, .. } = expression else {
        return;
    };
    let Some(body) = body.definition() else {
        return;
    };
    let target = match role {
        FunctionRole::Free => Symbol::from_parts(ctx.package_id(), name),
        FunctionRole::ImplMethod { type_name } => {
            Symbol::from_parts(ctx.package_id(), type_name).with_segment(name)
        }
        FunctionRole::InterfaceMethod { .. } => return,
    };

    if let Flow::SelfCall(span) = scan(body, target.as_str()) {
        ctx.sink
            .push(diagnostics::lint::unconditional_recursion(&span, name));
    }
}

#[derive(Clone, Copy)]
enum Flow {
    SelfCall(Span),
    Blocked,
    Continues,
}

fn scan(expression: &Expression, target: &str) -> Flow {
    match expression {
        Expression::Block { items, .. } => scan_sequence(items.iter(), target),
        Expression::Identifier { .. } | Expression::Unit { .. } | Expression::Lambda { .. } => {
            Flow::Continues
        }
        Expression::Literal { .. } | Expression::Tuple { .. } => {
            scan_sequence(expression.children().into_iter(), target)
        }
        Expression::Let { value, mode, .. } => sequence(scan(value, target), || {
            if matches!(mode, LetMode::Plain) {
                Flow::Continues
            } else {
                Flow::Blocked
            }
        }),
        Expression::Call {
            expression: callee,
            args,
            spread,
            ty,
            span,
            ..
        } => {
            let operands = std::iter::once(callee.as_ref())
                .chain(args)
                .chain(spread.as_deref());
            sequence(scan_sequence(operands, target), || {
                if resolved_definition(callee) == Some(target) {
                    Flow::SelfCall(*span)
                } else if returns_to_caller(ty) {
                    Flow::Continues
                } else {
                    Flow::Blocked
                }
            })
        }
        Expression::Binary {
            operator: BinaryOperator::And | BinaryOperator::Or,
            left,
            right,
            ..
        } => sequence(scan(left, target), || skippable(scan(right, target))),
        Expression::Binary { .. }
        | Expression::Unary { .. }
        | Expression::Paren { .. }
        | Expression::Cast { .. }
        | Expression::Reference { .. }
        | Expression::DotAccess { .. }
        | Expression::IndexedAccess { .. }
        | Expression::Range { .. } => scan_sequence(expression.children().into_iter(), target),
        // Emitted field evaluation order is not pinned to source order.
        Expression::StructCall { .. } | Expression::Assignment { .. } => {
            scan_unordered(expression.children().into_iter(), target)
        }
        Expression::Return { expression, .. }
        | Expression::Propagate { expression, .. }
        | Expression::Assert { expression, .. } => {
            sequence(scan(expression, target), || Flow::Blocked)
        }
        Expression::If {
            condition,
            consequence,
            alternative,
            ..
        } => sequence(scan(condition, target), || match alternative {
            Some(alternative) => join(scan(consequence, target), scan(alternative, target)),
            None => skippable(scan(consequence, target)),
        }),
        Expression::IfLet {
            scrutinee,
            consequence,
            alternative,
            ..
        } => sequence(scan(scrutinee, target), || match alternative.expression() {
            Some(alternative) => join(scan(consequence, target), scan(alternative, target)),
            None => skippable(scan(consequence, target)),
        }),
        Expression::Match { subject, arms, .. } => {
            sequence(scan(subject, target), || scan_arms(arms, target))
        }
        Expression::While { condition, .. } => sequence(scan(condition, target), || Flow::Blocked),
        Expression::WhileLet { scrutinee, .. } => {
            sequence(scan(scrutinee, target), || Flow::Blocked)
        }
        Expression::For { iterable, .. } => sequence(scan(iterable, target), || Flow::Blocked),
        Expression::Defer { expression, .. } | Expression::Task { expression, .. } => {
            scan_registered_call(expression, target)
        }
        _ => Flow::Blocked,
    }
}

// A deferred self-call stays unreported because it only runs if the function exits.
fn scan_registered_call(expression: &Expression, target: &str) -> Flow {
    match expression.unwrap_parens() {
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => {
            let operands = std::iter::once(callee.as_ref())
                .chain(args)
                .chain(spread.as_deref());
            scan_sequence(operands, target)
        }
        Expression::Block { .. } => Flow::Continues,
        _ => Flow::Blocked,
    }
}

fn scan_arms(arms: &[MatchArm], target: &str) -> Flow {
    let mut joined: Option<Flow> = None;
    for arm in arms {
        // Only the first guard is unconditional.
        if let Some(guard) = &arm.guard
            && !matches!(scan(guard, target), Flow::Continues)
        {
            return Flow::Blocked;
        }
        let flow = scan(&arm.expression, target);
        joined = Some(match joined {
            Some(accumulated) => join(accumulated, flow),
            None => flow,
        });
    }
    joined.unwrap_or(Flow::Blocked)
}

fn scan_sequence<'a>(items: impl Iterator<Item = &'a Expression>, target: &str) -> Flow {
    let mut flow = Flow::Continues;
    for item in items {
        flow = sequence(flow, || scan(item, target));
        if !matches!(flow, Flow::Continues) {
            break;
        }
    }
    flow
}

fn scan_unordered<'a>(items: impl Iterator<Item = &'a Expression>, target: &str) -> Flow {
    let mut self_call = None;
    for item in items {
        match scan(item, target) {
            Flow::Continues => continue,
            Flow::Blocked => return Flow::Blocked,
            Flow::SelfCall(span) => {
                self_call.get_or_insert(span);
            }
        }
    }
    match self_call {
        Some(span) => Flow::SelfCall(span),
        None => Flow::Continues,
    }
}

fn sequence(first: Flow, rest: impl FnOnce() -> Flow) -> Flow {
    match first {
        Flow::Continues => rest(),
        flow => flow,
    }
}

fn join(left: Flow, right: Flow) -> Flow {
    match (left, right) {
        (Flow::Blocked, _) | (_, Flow::Blocked) => Flow::Blocked,
        (Flow::SelfCall(span), Flow::SelfCall(_)) => Flow::SelfCall(span),
        _ => Flow::Continues,
    }
}

fn skippable(flow: Flow) -> Flow {
    join(flow, Flow::Continues)
}

// An erroneous type means an unknown callee, which cannot be assumed to return.
fn returns_to_caller(ty: &Type) -> bool {
    !ty.is_never() && !ty.contains_error()
}
