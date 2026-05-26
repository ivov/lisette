use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression};

pub fn check_self_comparison(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
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

    let (
        Expression::Identifier {
            value: left_name, ..
        },
        Expression::Identifier {
            value: right_name, ..
        },
    ) = (left.unwrap_parens(), right.unwrap_parens())
    else {
        return;
    };

    if left_name != right_name {
        return;
    }

    // NaN == NaN is false per IEEE 754, so skip floats.
    if left.get_type().is_float() {
        return;
    }

    let always_true = matches!(operator, Equal | LessThanOrEqual | GreaterThanOrEqual);
    diagnostics.push(diagnostics::lint::tautological_comparison(
        span,
        always_true,
    ));
}
