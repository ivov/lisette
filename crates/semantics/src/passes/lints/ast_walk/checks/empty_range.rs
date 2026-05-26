use diagnostics::LisetteDiagnostic;
use syntax::ast::{Expression, Literal, UnaryOperator};

pub fn check_empty_range(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Range {
        start: Some(start),
        end: Some(end),
        span,
        ..
    } = expression
    else {
        return;
    };

    let Some(start_value) = signed_integer_literal(start.unwrap_parens()) else {
        return;
    };
    let Some(end_value) = signed_integer_literal(end.unwrap_parens()) else {
        return;
    };

    if start_value > end_value {
        diagnostics.push(diagnostics::lint::empty_range(span));
    }
}

fn signed_integer_literal(expression: &Expression) -> Option<i128> {
    match expression {
        Expression::Literal {
            literal: Literal::Integer { value, .. },
            ..
        } => Some(*value as i128),
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => {
            let Expression::Literal {
                literal: Literal::Integer { value, .. },
                ..
            } = expression.unwrap_parens()
            else {
                return None;
            };
            Some(-(*value as i128))
        }
        _ => None,
    }
}
