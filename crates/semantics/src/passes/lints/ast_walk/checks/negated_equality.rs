use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};

pub fn check_negated_equality(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
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

    diagnostics.push(diagnostics::lint::negated_equality(span, is_equal));
}
