use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression};
use syntax::types::{CompoundKind, Type};

use super::helpers::{flip_comparison, is_zero_literal};

pub fn check_manual_is_empty(expression: &Expression, ctx: &NodeCtx) {
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

    ctx.sink
        .push(diagnostics::lint::manual_is_empty(span, &replacement));
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
