use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Span};

pub fn check_loop_runs_once(expression: &Expression, ctx: &NodeCtx) {
    let (body, span, keyword_len) = match expression {
        Expression::Loop { body, span, .. } => (body.as_ref(), span, 4),
        Expression::While { body, span, .. } | Expression::WhileLet { body, span, .. } => {
            (body.as_ref(), span, 5)
        }
        Expression::For { body, span, .. } => (body.as_ref(), span, 3),
        _ => return,
    };

    if always_exits(body) && !reenters_loop(body) {
        let keyword = Span::new(span.file_id, span.byte_offset, keyword_len);
        ctx.sink.push(diagnostics::lint::loop_runs_once(&keyword));
    }
}

/// True when every path through `expression` reaches a `break`, `return`, or
/// diverging call. A reachable `continue` is vetoed separately by `reenters_loop`.
fn always_exits(expression: &Expression) -> bool {
    match expression {
        Expression::Break { .. } | Expression::Return { .. } => true,
        Expression::Continue { .. } => false,
        Expression::Call { ty, .. } => ty.is_never(),
        Expression::Paren { expression, .. } | Expression::Cast { expression, .. } => {
            always_exits(expression)
        }
        Expression::If {
            consequence,
            alternative,
            ..
        }
        | Expression::IfLet {
            consequence,
            alternative,
            ..
        } => always_exits(consequence) && always_exits(alternative),
        Expression::Match { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|arm| always_exits(&arm.expression))
        }
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => block_exits(items),
        Expression::Loop { .. }
        | Expression::While { .. }
        | Expression::WhileLet { .. }
        | Expression::For { .. }
        | Expression::Function { .. }
        | Expression::Lambda { .. }
        | Expression::Task { .. } => false,
        _ => false,
    }
}

/// A block reaches an exit when its first diverging statement is itself an exit.
fn block_exits(items: &[Expression]) -> bool {
    for item in items {
        if item.diverges().is_some() {
            return always_exits(item);
        }
    }
    false
}

/// True when `expression` can reach a `continue` targeting this loop, including in
/// a nested loop's header (iterable, condition, scrutinee) but not its body.
fn reenters_loop(expression: &Expression) -> bool {
    match expression {
        Expression::Continue { .. } => true,
        Expression::For { iterable, .. } => reenters_loop(iterable),
        Expression::While { condition, .. } => reenters_loop(condition),
        Expression::WhileLet { scrutinee, .. } => reenters_loop(scrutinee),
        Expression::Loop { .. }
        | Expression::Function { .. }
        | Expression::Lambda { .. }
        | Expression::Task { .. } => false,
        _ => expression.children().into_iter().any(reenters_loop),
    }
}
