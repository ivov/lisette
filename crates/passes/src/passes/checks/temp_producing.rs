use crate::passes::walk::NodeCtx;
use diagnostics::LocalSink;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::program::ReceiverCoercion;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let sink = ctx.sink;
    match expression {
        Expression::Call { args, spread, .. } => {
            for arg in args {
                flag_sub_expression(arg, sink);
            }
            if let Some(s) = spread.as_ref() {
                flag_sub_expression(s, sink);
            }
        }
        Expression::StructCall {
            field_assignments, ..
        } => {
            for f in field_assignments {
                flag_sub_expression(&f.value, sink);
            }
        }
        Expression::Binary { left, right, .. } => {
            flag_sub_expression(left, sink);
            flag_sub_expression(right, sink);
        }
        Expression::Unary { expression, .. } | Expression::Reference { expression, .. } => {
            flag_sub_expression(expression, sink);
        }
        Expression::Cast { expression, .. } => {
            flag_sub_expression(expression, sink);
        }
        Expression::If { condition, .. } | Expression::While { condition, .. } => {
            flag_sub_expression(condition, sink);
        }
        Expression::IndexedAccess { index, .. } => {
            flag_sub_expression(index, sink);
        }
        Expression::Range { start, end, .. } => {
            if let Some(s) = start {
                flag_sub_expression(s, sink);
            }
            if let Some(e) = end {
                flag_sub_expression(e, sink);
            }
        }
        Expression::Literal {
            literal: Literal::Slice(elements),
            ..
        } => {
            for e in elements {
                flag_sub_expression(e, sink);
            }
        }
        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => {
            for p in parts {
                if let FormatStringPart::Expression(e) = p {
                    flag_sub_expression(e, sink);
                }
            }
        }
        _ => {}
    }
}

fn flag_sub_expression(expression: &Expression, sink: &LocalSink) {
    if expression.is_temp_producing() || has_auto_address_on_call(expression) {
        sink.push(diagnostics::infer::complex_sub_expression(
            expression.get_span(),
        ));
    }
}

fn has_auto_address_on_call(expression: &Expression) -> bool {
    let expression = expression.unwrap_parens();
    if let Expression::Call { expression, .. } = expression
        && let Expression::DotAccess {
            expression: receiver,
            receiver_coercion,
            ..
        } = expression.unwrap_parens()
    {
        if matches!(receiver.unwrap_parens(), Expression::Call { .. })
            && *receiver_coercion == Some(ReceiverCoercion::AutoAddress)
        {
            return true;
        }
        return has_auto_address_on_call(receiver);
    }
    false
}
