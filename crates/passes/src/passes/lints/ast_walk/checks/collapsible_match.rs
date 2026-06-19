use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};

use super::helpers::{
    expressions_equivalent, is_bare_identifier, mentions_identifier, unwrap_block,
};

pub fn check_collapsible_match(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match { arms, span, .. } = expression else {
        return;
    };

    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return;
    }

    // The collapsed form keeps the dismissal as `_ => ...`, so the outer one must
    // already be a trailing wildcard for the rewrite to stay exhaustive.
    let (meaningful, dismissal) = (&arms[0], &arms[1]);
    if !matches!(dismissal.pattern, Pattern::WildCard { .. }) {
        return;
    }
    let Some(binding) = single_binding_variant(&meaningful.pattern) else {
        return;
    };

    let Expression::Match {
        subject: inner_subject,
        arms: inner_arms,
        origin: inner_origin,
        span: inner_span,
        ..
    } = unwrap_block(&meaningful.expression)
    else {
        return;
    };

    if !is_bare_identifier(inner_subject, binding) {
        return;
    }
    if inner_arms.len() != 2 || inner_arms.iter().any(MatchArm::has_guard) {
        return;
    }

    // The inner dismissal must be a bare wildcard, not an identifier catch-all: an
    // identifier binds the matched value and may return it (`other => other`), which
    // the collapsed outer `_` arm cannot reference.
    let (inner_meaningful, inner_dismissal) = (&inner_arms[0], &inner_arms[1]);
    if !matches!(inner_dismissal.pattern, Pattern::WildCard { .. })
        || is_catch_all(&inner_meaningful.pattern)
    {
        return;
    }

    if !dismissals_equivalent(
        unwrap_block(&dismissal.expression),
        unwrap_block(&inner_dismissal.expression),
    ) {
        return;
    }

    // Merging drops the outer binding, so nothing kept may still refer to it.
    if mentions_identifier(&inner_meaningful.expression, binding)
        || mentions_identifier(&inner_dismissal.expression, binding)
    {
        return;
    }

    // Claim both nodes so `match_as_if_let` does not also advise on a node the
    // merge removes.
    let mut claimed = ctx.claimed_spans.borrow_mut();
    claimed.insert(Span::new(span.file_id, span.byte_offset, 5));
    claimed.insert(Span::new(inner_span.file_id, inner_span.byte_offset, 5));

    let inner_keyword_len = match inner_origin {
        MatchOrigin::Explicit => 5,
        MatchOrigin::IfLet { .. } => 2,
    };
    let inner_keyword_span = Span::new(
        inner_span.file_id,
        inner_span.byte_offset,
        inner_keyword_len,
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
