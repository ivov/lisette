use crate::passes::comparison::expressions_equivalent;
use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal, Span};
use syntax::constant::Value;
use syntax::types::Type;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::IndexedAccess {
            expression: receiver,
            index,
            from_colon_syntax: false,
            span,
            ..
        } => {
            let brackets = span.after(receiver.get_span());
            if let Expression::Range {
                start,
                end,
                inclusive,
                ..
            } = index.unwrap_parens()
            {
                let bounds = SliceBounds {
                    start: start.as_deref(),
                    end: end.as_deref(),
                    inclusive: *inclusive,
                };
                check_slice_bounds(receiver, bounds, &brackets, ctx);
            } else {
                check_index(receiver, index, &brackets, ctx);
            }
        }
        Expression::Call {
            expression: callee,
            args,
            span,
            ..
        } => {
            if let Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } = callee.unwrap_parens()
                && member == "byte_at"
                && ctx.store.peel_alias(&receiver.get_type()).is_string()
                && let [index] = args.as_slice()
            {
                let length = constant_length(receiver, ctx);
                check_constant_index(constant_index(index, ctx), length, span, ctx);
            }
        }
        _ => {}
    }
}

struct SliceBounds<'a> {
    start: Option<&'a Expression>,
    end: Option<&'a Expression>,
    inclusive: bool,
}

fn check_index(receiver: &Expression, index: &Expression, brackets: &Span, ctx: &NodeCtx) {
    let receiver_ty = ctx.store.peel_alias(&receiver.get_type());
    let indexable = matches!(receiver_ty, Type::Array { .. }) || receiver_ty.is_slice();
    if !indexable {
        return;
    }
    let length = match (&receiver_ty, receiver.unwrap_parens()) {
        (Type::Array { length, .. }, _) => Some(i128::from(*length)),
        (
            _,
            Expression::Literal {
                literal: Literal::Slice(elements),
                ..
            },
        ) => i128::try_from(elements.len()).ok(),
        _ => None,
    };
    if check_constant_index(constant_index(index, ctx), length, brackets, ctx) {
        return;
    }

    if receiver_ty.is_slice()
        && let Expression::Call {
            expression: callee,
            args,
            ..
        } = index.unwrap_parens()
        && args.is_empty()
        && let Expression::DotAccess {
            expression: call_receiver,
            member,
            ..
        } = callee.unwrap_parens()
        && member == "length"
        && is_dotted_identifier(receiver)
        && expressions_equivalent(receiver, call_receiver)
    {
        let receiver_text = receiver.root_identifier().unwrap_or("xs");
        ctx.sink.push(diagnostics::infer::index_out_of_bounds(
            brackets,
            &format!("{receiver_text}.length()"),
        ));
    }
}

fn check_constant_index(
    index: Option<i128>,
    length: Option<i128>,
    span: &Span,
    ctx: &NodeCtx,
) -> bool {
    let Some(index) = index else {
        return false;
    };
    if index < 0 {
        ctx.sink
            .push(diagnostics::infer::negative_index(span, &index.to_string()));
        return true;
    }
    if length.is_some_and(|length| index >= length) {
        ctx.sink.push(diagnostics::infer::index_out_of_bounds(
            span,
            &index.to_string(),
        ));
        return true;
    }
    false
}

fn check_slice_bounds(receiver: &Expression, bounds: SliceBounds, brackets: &Span, ctx: &NodeCtx) {
    let receiver_ty = ctx.store.peel_alias(&receiver.get_type());
    let length = match &receiver_ty {
        Type::Array { length, .. } => Some(i128::from(*length)),
        ty if ty.is_string() => constant_length(receiver, ctx),
        _ => None,
    };
    let start = bounds.start.and_then(|start| constant_index(start, ctx));
    let end = bounds
        .end
        .and_then(|end| constant_index(end, ctx))
        .and_then(|end| end.checked_add(i128::from(bounds.inclusive)));
    let bound_limit = length.map(|length| length + 1);
    for bound in [start, end] {
        if check_constant_index(bound, bound_limit, brackets, ctx) {
            return;
        }
    }
    let both_literals = [bounds.start, bounds.end]
        .into_iter()
        .all(|bound| bound.is_some_and(is_integer_literal));
    if let (Some(start), Some(end)) = (start, end)
        && start > end
        && !both_literals
    {
        ctx.sink.push(diagnostics::infer::slice_bounds_swapped(
            brackets,
            &start.to_string(),
            &end.to_string(),
        ));
    }
}

fn constant_index(index: &Expression, ctx: &NodeCtx) -> Option<i128> {
    ctx.evaluate(index).ok()??.value.as_integer()
}

fn constant_length(receiver: &Expression, ctx: &NodeCtx) -> Option<i128> {
    match ctx.evaluate(receiver).ok()??.value {
        Value::String(bytes) => i128::try_from(bytes.len()).ok(),
        _ => None,
    }
}

fn is_integer_literal(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Integer { .. },
            ..
        } => true,
        Expression::Unary { expression, .. } => is_integer_literal(expression),
        _ => false,
    }
}

fn is_dotted_identifier(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Identifier { .. } => true,
        Expression::DotAccess { expression, .. } => is_dotted_identifier(expression),
        _ => false,
    }
}
