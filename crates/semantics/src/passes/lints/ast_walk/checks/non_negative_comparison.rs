use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::program::CallKind;

pub fn check_non_negative_comparison(
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
        is_native_length_call(left.unwrap_parens()),
        is_native_length_call(right.unwrap_parens()),
    ) {
        (true, false) if is_zero_literal(right.unwrap_parens()) => *operator,
        (false, true) if is_zero_literal(left.unwrap_parens()) => flip_comparison(*operator),
        _ => return,
    };

    let always_true = match operator {
        LessThan => false,
        GreaterThanOrEqual => true,
        _ => return,
    };

    diagnostics.push(diagnostics::lint::non_negative_comparison(
        span,
        always_true,
    ));
}

fn is_native_length_call(expression: &Expression) -> bool {
    let Expression::Call {
        expression: callee,
        call_kind,
        ..
    } = expression
    else {
        return false;
    };

    if !matches!(
        call_kind,
        Some(CallKind::NativeMethod(_) | CallKind::NativeMethodIdentifier(_))
    ) {
        return false;
    }

    native_method_name(callee.unwrap_parens()) == Some("length")
}

fn native_method_name(callee: &Expression) -> Option<&str> {
    match callee {
        Expression::DotAccess { member, .. } => Some(member),
        Expression::Identifier { value, .. } => {
            Some(value.split_once('.').map_or(value, |(_, m)| m))
        }
        _ => None,
    }
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
        GreaterThan => LessThan,
        LessThanOrEqual => GreaterThanOrEqual,
        GreaterThanOrEqual => LessThanOrEqual,
        other => other,
    }
}
