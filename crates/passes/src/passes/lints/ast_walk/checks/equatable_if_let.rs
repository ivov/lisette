use crate::passes::walk::NodeCtx;
use semantics::checker::{TypeEnv, check_not_comparable};
use syntax::ast::{Expression, MatchOrigin, Pattern, Span};

use super::helpers::{enum_has_multiple_variants, span_text};

pub fn check_equatable_if_let(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match {
        subject,
        arms,
        origin,
        span,
        ..
    } = expression
    else {
        return;
    };

    if !matches!(origin, MatchOrigin::IfLet { .. }) || arms.len() != 2 {
        return;
    }

    let meaningful = &arms[0];
    if meaningful.has_guard() || !is_unit_variant_pattern(&meaningful.pattern) {
        return;
    }

    // The subject text is reused as the left operand, so it must be a primary
    // expression: an identifier or field access never carries an operator looser
    // than `==`, but a call can (a pipeline `|>` desugars to one).
    if !matches!(
        subject.unwrap_parens(),
        Expression::Identifier { .. } | Expression::DotAccess { .. }
    ) {
        return;
    }

    let subject_ty = ctx.store.deep_resolve_alias(&subject.get_type());

    // A fieldless variant has no inner value to coerce, so its resolved type equals
    // the subject's only when the if-let type-checks; this keeps a checker-rejected
    // pattern (wrong enum, missing variant) from being rewritten.
    let pattern_ty = meaningful
        .pattern
        .get_type()
        .map(|ty| ctx.store.deep_resolve_alias(&ty));
    if pattern_ty.as_ref() != Some(&subject_ty) {
        return;
    }

    if !enum_has_multiple_variants(&subject_ty, ctx.store) {
        return;
    }

    // Skip if the subject does not accept `==`, so the suggestion is never rejected.
    if check_not_comparable(&TypeEnv::default(), ctx.store, &subject_ty).is_some() {
        return;
    }

    let pattern_span = meaningful.pattern.get_span();
    let (Some(pattern_text), Some(subject_text)) = (
        ctx.source
            .get(pattern_span.byte_offset as usize..pattern_span.end() as usize),
        span_text(ctx.source, subject),
    ) else {
        return;
    };

    let if_keyword_span = Span::new(span.file_id, span.byte_offset, 2);
    ctx.sink.push(diagnostics::lint::equatable_if_let(
        &if_keyword_span,
        pattern_text,
        subject_text,
    ));
}

// Fielded variants and literals are excluded: an inner value can mismatch its
// field type without the outer pattern type showing it.
fn is_unit_variant_pattern(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::EnumVariant { identifier, fields, rest, .. }
        if !*rest && fields.is_empty() && identifier.contains('.'))
}
