use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Pattern, Span};

pub fn check_match_single_binding(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match { arms, span, .. } = expression else {
        return;
    };

    let [arm] = arms.as_slice() else {
        return;
    };

    if arm.has_guard() {
        return;
    }

    let Pattern::Identifier { identifier, .. } = &arm.pattern else {
        return;
    };

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink.push(diagnostics::lint::match_single_binding(
        &match_keyword_span,
        identifier,
    ));
}
