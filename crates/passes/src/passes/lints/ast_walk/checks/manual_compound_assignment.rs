use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::{expressions_equivalent, is_side_effect_free, span_text};

pub fn check_manual_compound_assignment(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Assignment {
        target,
        value,
        compound_operator: None,
        span,
    } = expression
    else {
        return;
    };

    let Expression::Binary {
        operator,
        left,
        right,
        ..
    } = value.unwrap_parens()
    else {
        return;
    };

    let Some(symbol) = operator.compound_assignment_symbol() else {
        return;
    };

    if !is_side_effect_free(target) || !expressions_equivalent(target, left) {
        return;
    }

    let mut diagnostic = diagnostics::lint::manual_compound_assignment(span, symbol);

    if let (Some(target_text), Some(rhs_text)) =
        (span_text(ctx.source, target), span_text(ctx.source, right))
    {
        let replacement = format!("{target_text} {symbol} {rhs_text}");
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Use `{symbol}`"),
            Edit::replacement(*span, replacement),
        ));
    }

    ctx.sink.push(diagnostic);
}
