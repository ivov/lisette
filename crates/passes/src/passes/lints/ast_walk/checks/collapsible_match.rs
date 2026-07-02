use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, MatchArm, Pattern, Span};

use super::helpers::{
    expressions_equivalent, is_bare_identifier, mentions_identifier, replacement_drops_comment,
    span_text, unwrap_block,
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
    let mut diagnostic = diagnostics::lint::collapsible_match(&inner_keyword_span);
    if matches!(expression, Expression::Match { .. })
        && let Some(fix) = merge_fix(ctx.source, &outer, &inner)
    {
        diagnostic = diagnostic.with_fix(fix);
    }
    ctx.sink.push(diagnostic);
}

fn merge_fix(source: &str, outer: &TwoArm, inner: &TwoArm) -> Option<Fix> {
    let Pattern::EnumVariant { fields, .. } = outer.meaningful_pattern else {
        return None;
    };
    let [binding] = fields.as_slice() else {
        return None;
    };

    let outer_pattern = outer.meaningful_pattern.get_span();
    let binding_span = binding.get_span();
    let inner_pattern = inner.meaningful_pattern.get_span();
    let inner_pattern_text =
        source.get(inner_pattern.byte_offset as usize..inner_pattern.end() as usize)?;
    let before =
        source.get(outer_pattern.byte_offset as usize..binding_span.byte_offset as usize)?;
    let after = source.get(binding_span.end() as usize..outer_pattern.end() as usize)?;
    let merged_pattern = format!("{before}{inner_pattern_text}{after}");

    let inner_body = span_text(source, inner.meaningful_expr)?;
    let arm = format!("{merged_pattern} => {inner_body}");

    let arm_span = Span::new(
        outer_pattern.file_id,
        outer_pattern.byte_offset,
        outer.meaningful_expr.get_span().end() - outer_pattern.byte_offset,
    );
    if replacement_drops_comment(source, arm_span, &arm) {
        return None;
    }
    Some(Fix::new(
        "Merge the inner pattern into the outer arm",
        Edit::replacement(arm_span, arm),
    ))
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
