use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::Expression;

use super::helpers::span_text;

pub fn check_excess_parens_on_condition(expression: &Expression, ctx: &NodeCtx) {
    let (condition, keyword) = match expression {
        Expression::If { condition, .. } => (condition.as_ref(), "if"),
        Expression::While { condition, .. } => (condition.as_ref(), "while"),
        Expression::Match { subject, .. } => (subject.as_ref(), "match"),
        Expression::IfLet { scrutinee, .. } => (scrutinee.as_ref(), "if let"),
        _ => return,
    };

    if let Expression::Paren {
        span, expression, ..
    } = condition
    {
        let mut diagnostic = diagnostics::lint::unnecessary_parens(span, keyword);
        if let Some(inner) = span_text(ctx.source, expression) {
            diagnostic = diagnostic.with_fix(Fix::new(
                "Remove redundant parentheses",
                Edit::replacement(*span, inner),
            ));
        }
        ctx.sink.push(diagnostic);
    }
}
