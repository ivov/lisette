use super::helpers::{is_zero_literal, span_text};
use crate::passes::walk::NodeCtx;
use semantics::store::Store;
use syntax::ast::{BinaryOperator, Expression, FormatStringPart, Literal, Span, UnaryOperator};
use syntax::program::{CallKind, NativeTypeKind};

pub fn check_manual_find(expression: &Expression, ctx: &NodeCtx) {
    let Some((span, filter_call, index)) = native_slice_method(expression, "get") else {
        return;
    };

    if !is_zero_literal(index.unwrap_parens()) {
        return;
    }

    let Some((_, receiver, predicate)) = native_slice_method(filter_call.unwrap_parens(), "filter")
    else {
        return;
    };

    if !predicate_is_pure(predicate, ctx.store) {
        return;
    }

    let (Some(receiver_text), Some(predicate_text)) = (
        span_text(ctx.source, receiver),
        span_text(ctx.source, predicate),
    ) else {
        return;
    };

    ctx.sink.push(diagnostics::lint::manual_find(
        span,
        receiver_text,
        predicate_text,
    ));
}

fn native_slice_method<'a>(
    expression: &'a Expression,
    name: &str,
) -> Option<(&'a Span, &'a Expression, &'a Expression)> {
    let Expression::Call {
        expression: callee,
        args,
        call_kind,
        span,
        ..
    } = expression
    else {
        return None;
    };

    if !matches!(
        call_kind,
        Some(CallKind::NativeMethod(NativeTypeKind::Slice))
    ) {
        return None;
    }

    let [arg] = args.as_slice() else {
        return None;
    };

    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };

    (member.as_str() == name).then_some((span, receiver.as_ref(), arg))
}

// `filter` evaluates the predicate on every element but `find` short-circuits, so
// the rewrite is sound only for a pure, non-panicking inline closure body.
fn predicate_is_pure(predicate: &Expression, store: &Store) -> bool {
    let Expression::Lambda { body, .. } = predicate.unwrap_parens() else {
        return false;
    };
    expression_is_pure(body, store)
}

fn expression_is_pure(expression: &Expression, store: &Store) -> bool {
    match expression.unwrap_parens() {
        Expression::Identifier { .. } => true,
        Expression::Literal { literal, .. } => literal_is_pure(literal, store),
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => false,
        Expression::Unary {
            expression: inner, ..
        } => expression_is_pure(inner, store),
        Expression::Binary {
            operator,
            left,
            right,
            ..
        } => {
            binary_operator_cannot_panic(*operator, left, right)
                && expression_is_pure(left, store)
                && expression_is_pure(right, store)
        }
        // Field access auto-derefs, which panics on a nilable Go pointer.
        Expression::DotAccess {
            expression: inner, ..
        } => !store.is_nilable_go_type(&inner.get_type()) && expression_is_pure(inner, store),
        Expression::Block { items, .. } => {
            matches!(items.as_slice(), [single] if expression_is_pure(single, store))
        }
        _ => false,
    }
}

// `/` `%` panic on zero, `<<` `>>` on a negative count, and interface/`any`
// `==`/`!=` on a non-comparable dynamic value; scalar `==`/`!=` is safe.
fn binary_operator_cannot_panic(
    operator: BinaryOperator,
    left: &Expression,
    right: &Expression,
) -> bool {
    match operator {
        BinaryOperator::Division
        | BinaryOperator::Remainder
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight => false,
        BinaryOperator::Equal | BinaryOperator::NotEqual => {
            left.get_type().underlying_simple_kind().is_some()
                && right.get_type().underlying_simple_kind().is_some()
        }
        _ => true,
    }
}

// An interpolated f-string lowers to `fmt.Sprint`, which can call a user `String()`.
fn literal_is_pure(literal: &Literal, store: &Store) -> bool {
    match literal {
        Literal::Slice(elements) => elements.iter().all(|e| expression_is_pure(e, store)),
        Literal::FormatString(parts) => parts
            .iter()
            .all(|part| matches!(part, FormatStringPart::Text(_))),
        _ => true,
    }
}
