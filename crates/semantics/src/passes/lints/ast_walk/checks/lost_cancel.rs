use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Pattern};
use syntax::types::Type;

pub fn check_lost_cancel(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Let { binding, value, .. } = expression else {
        return;
    };
    if !originates_from_call(value) {
        return;
    }
    check_slot(&binding.pattern, &binding.ty, ctx);
}

/// A call or a projection rooted in one produces a fresh cancel; a projection off
/// an existing binding (`pair.1`) is a copy that keeps the original alive.
fn originates_from_call(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Call { .. } => true,
        Expression::DotAccess { expression, .. } => originates_from_call(expression),
        _ => false,
    }
}

/// Fire when a cancel slot is discarded (`_`) or bound but never used. Calling it
/// on only some paths is a sound false negative, not a positive.
fn check_slot(pattern: &Pattern, ty: &Type, ctx: &NodeCtx) {
    match pattern {
        Pattern::Tuple { elements, .. } => {
            if let Type::Tuple(types) = ty
                && elements.len() == types.len()
            {
                for (element, element_ty) in elements.iter().zip(types) {
                    check_slot(element, element_ty, ctx);
                }
            }
        }
        Pattern::WildCard { span } if contains_cancel_func(ty) => {
            ctx.sink.push(diagnostics::lint::lost_cancel(span));
        }
        Pattern::Identifier { span, .. } if contains_cancel_func(ty) => {
            if ctx
                .facts
                .bindings
                .values()
                .any(|binding| binding.span == *span && !binding.used)
            {
                ctx.sink.push(diagnostics::lint::lost_cancel(span));
            }
        }
        _ => {}
    }
}

/// Whether `ty` is, or transitively contains through tuples and type aliases, a
/// cancel func.
fn contains_cancel_func(ty: &Type) -> bool {
    is_cancel_func(ty)
        || matches!(ty, Type::Tuple(elements) if elements.iter().any(contains_cancel_func))
        || ty.get_underlying().is_some_and(contains_cancel_func)
}

fn is_cancel_func(ty: &Type) -> bool {
    fn is_cancel_id(id: Option<&str>) -> bool {
        matches!(
            id,
            Some("go:context.CancelFunc" | "go:context.CancelCauseFunc")
        )
    }
    if ty.is_ref() {
        is_cancel_id(ty.strip_refs().get_qualified_id())
    } else {
        is_cancel_id(ty.get_qualified_id())
    }
}
