use diagnostics::LocalSink;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::program::ReceiverCoercion;

pub(super) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    match expression {
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => {
            for arg in args {
                check(arg, sink);
            }
            if let Some(s) = spread.as_ref() {
                check(s, sink);
            }
            visit_expression(callee, sink);
            for arg in args {
                visit_expression(arg, sink);
            }
            if let Some(s) = spread.as_ref() {
                visit_expression(s, sink);
            }
            return;
        }
        Expression::StructCall {
            field_assignments, ..
        } => {
            for f in field_assignments {
                check(&f.value, sink);
            }
        }
        Expression::Binary { left, right, .. } => {
            check(left, sink);
            check(right, sink);
        }
        Expression::Unary { expression, .. } | Expression::Reference { expression, .. } => {
            check(expression, sink);
        }
        Expression::Cast { expression, .. } => {
            check(expression, sink);
        }
        Expression::If { condition, .. } | Expression::While { condition, .. } => {
            check(condition, sink);
        }
        Expression::IndexedAccess { index, .. } => {
            check(index, sink);
        }
        Expression::Range { start, end, .. } => {
            if let Some(s) = start {
                check(s, sink);
            }
            if let Some(e) = end {
                check(e, sink);
            }
        }
        Expression::Literal {
            literal: Literal::Slice(elements),
            ..
        } => {
            for e in elements {
                check(e, sink);
            }
        }
        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => {
            for p in parts {
                if let FormatStringPart::Expression(e) = p {
                    check(e, sink);
                }
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, sink);
    }
}

fn check(expression: &Expression, sink: &LocalSink) {
    if is_temp_producing(expression) || has_auto_address_on_call(expression) {
        sink.push(diagnostics::infer::complex_sub_expression(
            expression.get_span(),
        ));
    }
}

pub(crate) fn is_temp_producing(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Block { .. }
            | Expression::Loop { .. }
            | Expression::Select { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. }
    )
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
