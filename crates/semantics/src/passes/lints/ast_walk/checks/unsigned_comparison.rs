use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression, Literal};

pub fn check_unsigned_comparison(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    use BinaryOperator::*;
    if !matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    ) {
        return;
    }

    let operator = match (
        is_zero_literal(left.unwrap_parens()),
        is_zero_literal(right.unwrap_parens()),
    ) {
        (true, false) if right.get_type().is_unsigned_int() => flip_comparison(*operator),
        (false, true) if left.get_type().is_unsigned_int() => *operator,
        _ => return,
    };

    let always_true = match operator {
        LessThan => false,
        GreaterThanOrEqual => true,
        _ => return,
    };

    diagnostics.push(diagnostics::lint::unsigned_comparison(span, always_true));
}

fn is_zero_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 0, .. },
            ..
        }
    )
}

fn flip_comparison(operator: BinaryOperator) -> BinaryOperator {
    use BinaryOperator::*;
    match operator {
        LessThan => GreaterThan,
        LessThanOrEqual => GreaterThanOrEqual,
        GreaterThan => LessThan,
        GreaterThanOrEqual => LessThanOrEqual,
        other => other,
    }
}
