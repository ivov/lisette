use diagnostics::LisetteDiagnostic;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::types::{CompoundKind, Type};

pub fn check_manual_is_empty(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
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

    let (receiver, operator) = match (
        length_call_receiver(left.unwrap_parens()),
        length_call_receiver(right.unwrap_parens()),
    ) {
        (Some(receiver), None) if is_zero_literal(right.unwrap_parens()) => (receiver, *operator),
        (None, Some(receiver)) if is_zero_literal(left.unwrap_parens()) => {
            (receiver, flip_comparison(*operator))
        }
        _ => return,
    };

    if !type_has_is_empty(&receiver.get_type()) {
        return;
    }

    let negate = match operator {
        Equal | LessThanOrEqual => false,
        NotEqual | GreaterThan => true,
        _ => return,
    };

    let Some(receiver_text) = receiver.as_dotted_path() else {
        return;
    };

    let replacement = if negate {
        format!("!{receiver_text}.is_empty()")
    } else {
        format!("{receiver_text}.is_empty()")
    };

    diagnostics.push(diagnostics::lint::manual_is_empty(span, &replacement));
}

fn length_call_receiver(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return None;
    };

    if !args.is_empty() {
        return None;
    }

    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };

    (member == "length").then_some(receiver.as_ref())
}

fn type_has_is_empty(ty: &Type) -> bool {
    ty.is_string()
        || ty.is_slice()
        || ty.is_map()
        || ty.is_channel()
        || ty.is_native(CompoundKind::Sender)
        || ty.is_receiver()
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
