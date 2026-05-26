use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression};

use crate::call_target::resolve_call;

pub fn check_nan_comparison(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
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

    if !is_math_nan_call(left.unwrap_parens()) && !is_math_nan_call(right.unwrap_parens()) {
        return;
    }

    let always_true = matches!(operator, NotEqual);
    diagnostics.push(diagnostics::lint::nan_comparison(span, always_true));
}

fn is_math_nan_call(expression: &Expression) -> bool {
    resolve_call(expression).is_some_and(|target| target.is("go:math", "NaN"))
}
