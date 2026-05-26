use diagnostics::LocalSink;
use syntax::ast::{Expression, Literal, UnaryOperator};

pub(crate) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    if let Expression::Range {
        start: Some(start),
        end: Some(end),
        span,
        ..
    } = expression
        && let Some(start_value) = signed_integer_literal(start.unwrap_parens())
        && let Some(end_value) = signed_integer_literal(end.unwrap_parens())
        && start_value > end_value
    {
        sink.push(diagnostics::infer::empty_range(span));
    }

    for child in expression.children() {
        visit_expression(child, sink);
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
