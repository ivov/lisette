use crate::passes::walk::{ClaimKind, NodeCtx};
use syntax::ast::{Expression, Literal, UnaryOperator};
use syntax::constant::{self, ConstantError, Value};
use syntax::types::{SimpleKind, Type};

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
    let span = expression.get_span();
    if ctx.is_claimed(ClaimKind::ConstantOverflow, &span) {
        return;
    }
    let evaluation = ctx.evaluate(expression);
    if matches!(evaluation, Ok(None)) {
        return;
    }

    // Go checks the range of the whole expression, so `(max + 1) - 1` is legal.
    claim_operands(expression, ctx);

    let ty = expression.get_type();
    if ty.contains_error()
        || is_negated_literal(expression)
        || is_checker_rejected_cast(expression, ctx)
    {
        return;
    }

    let error = match evaluation {
        Ok(Some(constant)) if constant.go_type.is_none() => {
            let Some(kind) = slot_kind(&ty, &constant.value, ctx) else {
                return;
            };
            match constant::represent(constant.value, kind, span) {
                Ok(_) => return,
                Err(error) => error,
            }
        }
        Ok(_) => return,
        Err(error) => error,
    };
    report(error, expression, &ty, ctx);
}

/// Marks a constant subtree, so no part of it is range checked on its own.
pub(super) fn claim_constant(expression: &Expression, ctx: &mut NodeCtx) {
    ctx.claim(ClaimKind::ConstantOverflow, expression.get_span());
    claim_operands(expression, ctx);
}

fn claim_operands(expression: &Expression, ctx: &mut NodeCtx) {
    if constant::builtin_length_receiver(expression).is_some() {
        return;
    }
    for child in expression.children() {
        ctx.claim(ClaimKind::ConstantOverflow, child.get_span());
        claim_operands(child, ctx);
    }
}

fn slot_kind(ty: &Type, value: &Value, ctx: &NodeCtx) -> Option<SimpleKind> {
    if let Some(kind) = ctx.store.underlying_simple_kind(ty)
        && kind != SimpleKind::Unit
    {
        return Some(kind);
    }
    if !ty.is_unknown() && !ctx.store.is_interface(ty) {
        return None;
    }
    Some(match value {
        Value::Integer(_) => SimpleKind::Int,
        Value::Rune(_) => SimpleKind::Rune,
        Value::Float(_) => SimpleKind::Float64,
        Value::Complex(..) => SimpleKind::Complex128,
        Value::Bool(_) | Value::String(_) => return None,
    })
}

fn report(error: ConstantError, expression: &Expression, ty: &Type, ctx: &NodeCtx) {
    let diagnostic = match error {
        ConstantError::Overflow {
            span,
            value,
            kind,
            in_conversion,
        } => {
            let Some((min, max)) = kind.integer_range() else {
                ctx.sink.push(diagnostics::infer::float_literal_overflow(
                    kind.leaf_name(),
                    span,
                ));
                return;
            };
            let at_root = span == expression.get_span();
            let type_name = if at_root {
                ty.to_string()
            } else {
                kind.leaf_name().to_string()
            };
            if at_root && is_lone_literal(expression) {
                diagnostics::infer::integer_literal_overflow(kind.sized_name(), min, max, span)
            } else if in_conversion {
                diagnostics::infer::constant_cast_overflow(
                    &span,
                    &type_name,
                    &value.to_string(),
                    min,
                    max,
                )
            } else {
                diagnostics::infer::constant_overflow(
                    &span,
                    &type_name,
                    &value.to_string(),
                    min,
                    max,
                )
            }
        }
        ConstantError::Truncated { span } => {
            diagnostics::infer::fractional_float_constant_int_cast(span)
        }
        ConstantError::TooLarge { span } => diagnostics::infer::constant_too_large(span),
        ConstantError::DivisionByZero { .. } | ConstantError::ShiftCount { .. } => return,
    };
    ctx.sink.push(diagnostic);
}

fn is_negated_literal(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => matches!(
            expression.unwrap_parens(),
            Expression::Literal {
                literal: Literal::Integer { .. },
                ..
            }
        ),
        Expression::Literal {
            literal: Literal::Integer {
                text: Some(text), ..
            },
            ..
        } => text.starts_with('-'),
        _ => false,
    }
}

fn is_lone_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Integer { .. } | Literal::Char(_),
            ..
        }
    )
}

fn is_checker_rejected_cast(expression: &Expression, ctx: &NodeCtx) -> bool {
    let Expression::Cast {
        expression: operand,
        ty,
        ..
    } = expression.unwrap_parens()
    else {
        return false;
    };
    let target_is_integer = ctx
        .store
        .underlying_simple_kind(ty)
        .is_some_and(|kind| kind.integer_range().is_some());
    (target_is_integer && is_float_literal(operand))
        || (ctx.store.has_underlying_rune(&operand.get_type()) && ctx.store.has_underlying_byte(ty))
}

fn is_float_literal(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Float { .. },
            ..
        } => true,
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => is_float_literal(expression),
        _ => false,
    }
}
