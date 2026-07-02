use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BinaryOperator, Expression, Span};

use super::helpers::{replacement_drops_comment, span_text};

pub fn check_collapsible_if(expression: &Expression, ctx: &NodeCtx) {
    let Expression::If {
        condition,
        consequence,
        alternative,
        span,
        ..
    } = expression
    else {
        return;
    };

    if !is_missing_else(alternative) {
        return;
    }

    let Expression::Block { items, .. } = consequence.as_ref() else {
        return;
    };
    let [
        Expression::If {
            condition: inner_condition,
            consequence: inner_consequence,
            alternative: inner_alternative,
            span: inner_span,
            ..
        },
    ] = items.as_slice()
    else {
        return;
    };
    if !is_missing_else(inner_alternative) {
        return;
    }

    if !condition.get_type().is_boolean() || !inner_condition.get_type().is_boolean() {
        return;
    }

    let inner_if_keyword_span = Span::new(inner_span.file_id, inner_span.byte_offset, 2);
    let mut diagnostic = diagnostics::lint::collapsible_if(&inner_if_keyword_span);
    if let (Some(outer), Some(inner), Some(body)) = (
        as_and_operand(ctx.source, condition),
        as_and_operand(ctx.source, inner_condition),
        span_text(ctx.source, inner_consequence),
    ) {
        let replacement = format!("if {outer} && {inner} {body}");
        if !replacement_drops_comment(ctx.source, *span, &replacement) {
            diagnostic = diagnostic.with_fix(Fix::new(
                format!("Collapse into `if {outer} && {inner}`"),
                Edit::replacement(*span, replacement),
            ));
        }
    }
    ctx.sink.push(diagnostic);
}

fn as_and_operand(source: &str, condition: &Expression) -> Option<String> {
    let inner = condition.unwrap_parens();
    let text = span_text(source, inner)?;
    let parenthesize = binds_looser_than_and(inner);
    Some(if parenthesize {
        format!("({text})")
    } else {
        text.to_string()
    })
}

fn binds_looser_than_and(expression: &Expression) -> bool {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::Or,
            ..
        } => true,
        Expression::Call {
            expression: callee,
            args,
            ..
        } => args
            .iter()
            .any(|arg| arg.get_span().byte_offset < callee.get_span().byte_offset),
        _ => false,
    }
}

fn is_missing_else(alternative: &Expression) -> bool {
    matches!(alternative, Expression::Unit { .. })
}
