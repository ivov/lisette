use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal};

pub fn check_match_literal_collection(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match { subject, .. } = expression else {
        return;
    };

    let unwrapped = subject.unwrap_parens();

    if !unwrapped.is_all_literals() {
        return;
    }

    let span = match unwrapped {
        Expression::Literal {
            literal: Literal::Slice(_),
            span,
            ..
        } => Some(span),
        Expression::Tuple { span, .. } => Some(span),
        _ => None,
    };

    if let Some(span) = span {
        ctx.sink.push(diagnostics::lint::match_on_literal(span));
    }
}
