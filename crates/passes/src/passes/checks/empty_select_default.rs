use diagnostics::LocalSink;
use syntax::ast::{Expression, SelectArmPattern};

pub(crate) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, false, sink);
    }
}

fn visit_expression(expression: &Expression, in_loop: bool, sink: &LocalSink) {
    if in_loop && let Expression::Select { arms, .. } = expression {
        for arm in arms {
            if let SelectArmPattern::WildCard { body } = &arm.pattern
                && is_empty_body(body)
            {
                sink.push(diagnostics::infer::empty_select_default(&body.get_span()));
                break;
            }
        }
    }

    match expression {
        Expression::Function { body, .. } | Expression::Lambda { body, .. } => {
            visit_expression(body, false, sink);
        }
        Expression::Task {
            expression: inner, ..
        }
        | Expression::Defer {
            expression: inner, ..
        } => {
            visit_expression(inner, false, sink);
        }
        Expression::Loop { body, .. } => {
            visit_expression(body, true, sink);
        }
        Expression::While {
            condition, body, ..
        } => {
            visit_expression(condition, true, sink);
            visit_expression(body, true, sink);
        }
        Expression::WhileLet {
            scrutinee, body, ..
        } => {
            visit_expression(scrutinee, true, sink);
            visit_expression(body, true, sink);
        }
        Expression::For { iterable, body, .. } => {
            visit_expression(iterable, in_loop, sink);
            visit_expression(body, true, sink);
        }
        _ => {
            for child in expression.children() {
                visit_expression(child, in_loop, sink);
            }
        }
    }
}

fn is_empty_body(expression: &Expression) -> bool {
    matches!(expression, Expression::Block { items, .. } if items.is_empty())
        || matches!(expression, Expression::Unit { .. })
}
