use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal, MatchOrigin, Pattern, Span};

use super::helpers::expressions_equivalent;

pub fn check_match_on_bool(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match {
        arms,
        origin: MatchOrigin::Explicit,
        span,
        ..
    } = expression
    else {
        return;
    };

    let [first, second] = arms.as_slice() else {
        return;
    };

    if first.has_guard() || second.has_guard() {
        return;
    }

    let (Some(first_bool), Some(second_bool)) =
        (bool_pattern(&first.pattern), bool_pattern(&second.pattern))
    else {
        return;
    };

    if first_bool == second_bool {
        return;
    }

    if expressions_equivalent(&first.expression, &second.expression) {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink
        .push(diagnostics::lint::match_on_bool(&match_keyword_span));
}

fn bool_pattern(pattern: &Pattern) -> Option<bool> {
    match pattern {
        Pattern::Literal {
            literal: Literal::Boolean(value),
            ..
        } => Some(*value),
        _ => None,
    }
}
