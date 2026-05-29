use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::{BindingId, Expression, UnaryOperator};

use crate::facts::BindingFact;

pub(crate) fn run(
    typed_ast: &[Expression],
    bindings: &HashMap<BindingId, BindingFact>,
    sink: &LocalSink,
) {
    for item in typed_ast {
        visit_expression(item, bindings, sink);
    }
}

fn visit_expression(
    expression: &Expression,
    bindings: &HashMap<BindingId, BindingFact>,
    sink: &LocalSink,
) {
    if let Expression::While {
        condition, body, ..
    } = expression
        && condition_is_unchanging(condition, bindings)
        && !body_has_exit(body)
    {
        sink.push(diagnostics::infer::unchanging_loop_condition(
            &condition.get_span(),
        ));
    }

    for child in expression.children() {
        visit_expression(child, bindings, sink);
    }
}

/// True when the condition references a binding and is built only from
/// never-mutated bindings, literals, and pure operators. Member, index, and
/// pointer-deref access are rejected: they read through aliases that the
/// per-binding mutation facts do not track.
///
/// `mutated` is whole-program, not loop-scoped: a binding assigned outside the
/// loop also suppresses the check. That is a deliberate sound under-approximation
/// (loop-scoped invariance cannot be decided from this fact without false
/// positives), so the check misses loops whose variable is changed only outside
/// the body but never wrongly flags a loop that can terminate.
fn condition_is_unchanging(
    condition: &Expression,
    bindings: &HashMap<BindingId, BindingFact>,
) -> bool {
    let mut references_binding = false;
    is_invariant(condition, bindings, &mut references_binding) && references_binding
}

fn is_invariant(
    expression: &Expression,
    bindings: &HashMap<BindingId, BindingFact>,
    references_binding: &mut bool,
) -> bool {
    match expression {
        Expression::Identifier {
            binding_id: Some(id),
            ..
        } => match bindings.get(id) {
            Some(fact) if !fact.mutated => {
                *references_binding = true;
                true
            }
            _ => false,
        },
        Expression::Identifier { .. } => false,
        Expression::Literal { .. } => expression
            .children()
            .into_iter()
            .all(|child| is_invariant(child, bindings, references_binding)),
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => false,
        Expression::Paren { expression, .. } | Expression::Unary { expression, .. } => {
            is_invariant(expression, bindings, references_binding)
        }
        Expression::Binary { left, right, .. } => {
            is_invariant(left, bindings, references_binding)
                && is_invariant(right, bindings, references_binding)
        }
        _ => false,
    }
}

fn body_has_exit(body: &Expression) -> bool {
    body.contains_break() || contains_function_exit(body, false)
}

fn contains_function_exit(expression: &Expression, inside_try: bool) -> bool {
    match expression {
        Expression::Return { .. } => true,
        Expression::Propagate {
            expression: inner, ..
        } => !inside_try || contains_function_exit(inner, inside_try),
        Expression::Call { ty, .. } if ty.is_never() => true,
        Expression::Function { .. } | Expression::Lambda { .. } | Expression::Task { .. } => false,
        Expression::TryBlock { items, .. } => {
            items.iter().any(|item| contains_function_exit(item, true))
        }
        _ => expression
            .children()
            .into_iter()
            .any(|child| contains_function_exit(child, inside_try)),
    }
}
