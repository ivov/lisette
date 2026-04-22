use diagnostics::DiagnosticSink;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::program::{CoercionInfo, ReceiverCoercion};

pub(super) fn run(typed_ast: &[Expression], coercions: &CoercionInfo, sink: &DiagnosticSink) {
    for item in typed_ast {
        visit_expression(item, coercions, sink);
    }
}

fn visit_expression(expression: &Expression, coercions: &CoercionInfo, sink: &DiagnosticSink) {
    match expression {
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => {
            for arg in args {
                check(arg, coercions, sink);
            }
            if let Some(s) = spread.as_ref() {
                check(s, coercions, sink);
            }
            visit_expression(callee, coercions, sink);
            for arg in args {
                visit_expression(arg, coercions, sink);
            }
            if let Some(s) = spread.as_ref() {
                visit_expression(s, coercions, sink);
            }
            return;
        }
        Expression::StructCall {
            field_assignments, ..
        } => {
            for f in field_assignments {
                check(&f.value, coercions, sink);
            }
        }
        Expression::Binary { left, right, .. } => {
            check(left, coercions, sink);
            check(right, coercions, sink);
        }
        Expression::Unary { expression, .. } | Expression::Reference { expression, .. } => {
            check(expression, coercions, sink);
        }
        Expression::Cast { expression, .. } => {
            check(expression, coercions, sink);
        }
        Expression::If { condition, .. } | Expression::While { condition, .. } => {
            check(condition, coercions, sink);
        }
        Expression::IndexedAccess { index, .. } => {
            check(index, coercions, sink);
        }
        Expression::Range { start, end, .. } => {
            if let Some(s) = start {
                check(s, coercions, sink);
            }
            if let Some(e) = end {
                check(e, coercions, sink);
            }
        }
        Expression::Literal {
            literal: Literal::Slice(elements),
            ..
        } => {
            for e in elements {
                check(e, coercions, sink);
            }
        }
        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => {
            for p in parts {
                if let FormatStringPart::Expression(e) = p {
                    check(e, coercions, sink);
                }
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, coercions, sink);
    }
}

fn check(expression: &Expression, coercions: &CoercionInfo, sink: &DiagnosticSink) {
    if is_temp_producing(expression) || has_auto_address_on_call(expression, coercions) {
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

fn has_auto_address_on_call(expression: &Expression, coercions: &CoercionInfo) -> bool {
    let expression = expression.unwrap_parens();
    if let Expression::Call { expression, .. } = expression
        && let Expression::DotAccess {
            expression: receiver,
            ..
        } = expression.unwrap_parens()
    {
        if matches!(receiver.unwrap_parens(), Expression::Call { .. })
            && coercions.get_coercion(receiver.get_span()) == Some(ReceiverCoercion::AutoAddress)
        {
            return true;
        }
        return has_auto_address_on_call(receiver, coercions);
    }
    false
}
