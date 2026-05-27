use diagnostics::LocalSink;
use syntax::ast::Expression;

pub(crate) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    if let Expression::Loop { body, span, .. } = expression
        && let Expression::Block { items, .. } = body.as_ref()
        && items.is_empty()
    {
        sink.push(diagnostics::infer::empty_infinite_loop(span));
    }

    for child in expression.children() {
        visit_expression(child, sink);
    }
}
