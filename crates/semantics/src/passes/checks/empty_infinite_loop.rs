use diagnostics::LocalSink;
use syntax::ast::Expression;

pub(crate) fn check(expression: &Expression, sink: &LocalSink) {
    if let Expression::Loop { body, span, .. } = expression
        && let Expression::Block { items, .. } = body.as_ref()
        && items.is_empty()
    {
        sink.push(diagnostics::infer::empty_infinite_loop(span));
    }
}
