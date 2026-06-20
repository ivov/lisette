use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, Pattern, Span};

use super::helpers::{
    expressions_equivalent, is_bare_identifier, mentions_identifier, unwrap_block,
};

struct TwoArm<'a> {
    subject: &'a Expression,
    meaningful_pattern: &'a Pattern,
    meaningful_expr: &'a Expression,
    dismissal_is_wildcard: bool,
    dismissal_expr: &'a Expression,
    keyword_len: u32,
    span: Span,
}

fn as_two_arm(expression: &Expression) -> Option<TwoArm<'_>> {
    match expression {
        Expression::Match {
            subject,
            arms,
            span,
            ..
        } => {
            if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
                return None;
            }
            Some(TwoArm {
                subject: subject.as_ref(),
                meaningful_pattern: &arms[0].pattern,
                meaningful_expr: arms[0].expression.as_ref(),
                dismissal_is_wildcard: matches!(arms[1].pattern, Pattern::WildCard { .. }),
                dismissal_expr: arms[1].expression.as_ref(),
                keyword_len: 5,
                span: *span,
            })
        }
        Expression::IfLet {
            scrutinee,
            pattern,
            consequence,
            alternative,
            span,
            ..
        } => Some(TwoArm {
            subject: scrutinee.as_ref(),
            meaningful_pattern: pattern,
            meaningful_expr: consequence.as_ref(),
            dismissal_is_wildcard: true,
            dismissal_expr: alternative.as_ref(),
            keyword_len: 2,
            span: *span,
        }),
        _ => None,
    }
}

pub fn check_collapsible_match(expression: &Expression, ctx: &NodeCtx) {
    let Some(outer) = as_two_arm(expression) else {
        return;
    };

    // The collapsed form keeps the dismissal as `_ => ...`, so the outer one must
    // already be a trailing wildcard for the rewrite to stay exhaustive.
    if !outer.dismissal_is_wildcard {
        return;
    }
    let Some(binding) = single_binding_variant(outer.meaningful_pattern) else {
        return;
    };

    let Some(inner) = as_two_arm(unwrap_block(outer.meaningful_expr)) else {
        return;
    };

    if !is_bare_identifier(inner.subject, binding) {
        return;
    }

    // The inner dismissal must be a bare wildcard, not an identifier catch-all: an
    // identifier binds the matched value and may return it (`other => other`), which
    // the collapsed outer `_` arm cannot reference.
    if !inner.dismissal_is_wildcard || is_catch_all(inner.meaningful_pattern) {
        return;
    }

    if !dismissals_equivalent(
        unwrap_block(outer.dismissal_expr),
        unwrap_block(inner.dismissal_expr),
    ) {
        return;
    }

    // Merging drops the outer binding, so nothing kept may still refer to it.
    if mentions_identifier(inner.meaningful_expr, binding)
        || mentions_identifier(inner.dismissal_expr, binding)
    {
        return;
    }

    // Claim both nodes so `match_as_if_let` does not also advise on a node the
    // merge removes.
    let mut claimed = ctx.claimed_spans.borrow_mut();
    claimed.insert(Span::new(
        outer.span.file_id,
        outer.span.byte_offset,
        outer.keyword_len,
    ));
    claimed.insert(Span::new(
        inner.span.file_id,
        inner.span.byte_offset,
        inner.keyword_len,
    ));

    let inner_keyword_span = Span::new(
        inner.span.file_id,
        inner.span.byte_offset,
        inner.keyword_len,
    );
    ctx.sink
        .push(diagnostics::lint::collapsible_match(&inner_keyword_span));
}

fn single_binding_variant(pattern: &Pattern) -> Option<&str> {
    let Pattern::EnumVariant { fields, rest, .. } = pattern else {
        return None;
    };
    if *rest || fields.len() != 1 {
        return None;
    }
    let Pattern::Identifier { identifier, .. } = &fields[0] else {
        return None;
    };
    Some(identifier.as_str())
}

fn is_catch_all(pattern: &Pattern) -> bool {
    matches!(
        pattern,
        Pattern::WildCard { .. } | Pattern::Identifier { .. }
    )
}

fn dismissals_equivalent(a: &Expression, b: &Expression) -> bool {
    match (a, b) {
        (Expression::Unit { .. }, Expression::Unit { .. }) => true,
        (Expression::Continue { .. }, Expression::Continue { .. }) => true,
        (Expression::Break { value: a, .. }, Expression::Break { value: b, .. }) => match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => dismissals_equivalent(unwrap_block(a), unwrap_block(b)),
            _ => false,
        },
        (Expression::Return { expression: a, .. }, Expression::Return { expression: b, .. })
        | (
            Expression::Propagate { expression: a, .. },
            Expression::Propagate { expression: b, .. },
        ) => dismissals_equivalent(unwrap_block(a), unwrap_block(b)),
        _ => expressions_equivalent(a, b),
    }
}
