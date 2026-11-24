use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Literal, Span};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let (operator, left, right, span) = match expression {
        Expression::Binary {
            operator,
            left,
            right,
            span,
            ..
        } => (*operator, Some(left.as_ref()), right.as_ref(), *span),
        Expression::Assignment {
            compound_operator: Some(operator),
            value,
            ..
        } => (*operator, None, value.as_ref(), value.get_span()),
        _ => return,
    };
    if !matches!(
        operator,
        BinaryOperator::Division | BinaryOperator::Remainder
    ) {
        return;
    }
    report_zero_divisor(operator, left, right, span, ctx);
}

fn report_zero_divisor(
    operator: BinaryOperator,
    left: Option<&Expression>,
    right: &Expression,
    span: Span,
    ctx: &NodeCtx,
) {
    let is_zero = matches!(ctx.evaluate(right), Ok(Some(constant)) if constant.value.is_zero());
    let divisor_ty = right.get_type();
    if !is_zero || divisor_ty.contains_error() {
        return;
    }
    let ieee = ctx
        .store
        .underlying_simple_kind(&divisor_ty)
        .is_some_and(|kind| kind.integer_range().is_none());
    if ieee {
        let constant_dividend = left.is_some_and(|left| matches!(ctx.evaluate(left), Ok(Some(_))));
        if operator == BinaryOperator::Remainder || !(constant_dividend || is_literal(right)) {
            return;
        }
    }
    ctx.sink
        .push(diagnostics::infer::division_by_zero(span, ieee));
}

fn is_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Integer { .. } | Literal::Float { .. } | Literal::Imaginary(_),
            ..
        }
    )
}
