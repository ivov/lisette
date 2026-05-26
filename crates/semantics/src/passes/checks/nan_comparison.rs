use diagnostics::LocalSink;
use syntax::ast::{BinaryOperator, Expression};

use crate::call_target::resolve_call;

pub(crate) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    if let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    {
        use BinaryOperator::*;
        if matches!(
            operator,
            Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
        ) && (is_math_nan_call(left.unwrap_parens()) || is_math_nan_call(right.unwrap_parens()))
        {
            let always_true = matches!(operator, NotEqual);
            sink.push(diagnostics::infer::nan_comparison(span, always_true));
        }
    }

    for child in expression.children() {
        visit_expression(child, sink);
    }
}

fn is_math_nan_call(expression: &Expression) -> bool {
    resolve_call(expression).is_some_and(|target| target.is("go:math", "NaN"))
}
