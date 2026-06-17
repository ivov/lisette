use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};

pub fn check_negated_equality(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Unary {
        operator: UnaryOperator::Not,
        expression: operand,
        span,
        ..
    } = expression
    else {
        return;
    };

    let Expression::Binary { operator, .. } = operand.unwrap_parens() else {
        return;
    };

    let is_equal = match operator {
        BinaryOperator::Equal => true,
        BinaryOperator::NotEqual => false,
        _ => return,
    };

    ctx.sink
        .push(diagnostics::lint::negated_equality(span, is_equal));
}
