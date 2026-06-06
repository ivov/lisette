use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, SelectArmPattern, Span};

use super::helpers::span_text;

pub fn check_single_arm_select(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Select { arms, span, .. } = expression else {
        return;
    };

    let [arm] = arms.as_slice() else {
        return;
    };

    let SelectArmPattern::MatchReceive {
        receive_expression, ..
    } = &arm.pattern
    else {
        return;
    };

    let Some(receive_text) = span_text(ctx.source, receive_expression) else {
        return;
    };

    let select_keyword_span = Span::new(span.file_id, span.byte_offset, 6);
    ctx.sink.push(diagnostics::lint::single_arm_select(
        &select_keyword_span,
        receive_text,
    ));
}
