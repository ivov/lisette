use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use semantics::checker::{TypeEnv, check_not_comparable};
use syntax::ast::{Expression, Pattern, Span};

use super::helpers::{enum_has_multiple_variants, replacement_drops_comment, span_text};

pub fn check_equatable_if_let(expression: &Expression, ctx: &NodeCtx) {
    let Expression::IfLet {
        pattern,
        scrutinee,
        span,
        ..
    } = expression
    else {
        return;
    };

    if !is_unit_variant_pattern(pattern) {
        return;
    }

    // The subject text is reused as the left operand, so it must be a primary
    // expression: an identifier or field access never carries an operator looser
    // than `==`, but a call can (a pipeline `|>` desugars to one).
    if !matches!(
        scrutinee.unwrap_parens(),
        Expression::Identifier { .. } | Expression::DotAccess { .. }
    ) {
        return;
    }

    let subject_ty = ctx.store.deep_resolve_alias(&scrutinee.get_type());

    // A fieldless variant has no inner value to coerce, so its resolved type equals
    // the subject's only when the if-let type-checks; this keeps a checker-rejected
    // pattern (wrong enum, missing variant) from being rewritten.
    let pattern_ty = pattern
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

    let pattern_span = pattern.get_span();
    let (Some(pattern_text), Some(subject_text)) = (
        ctx.source
            .get(pattern_span.byte_offset as usize..pattern_span.end() as usize),
        span_text(ctx.source, scrutinee),
    ) else {
        return;
    };

    let if_keyword_span = Span::new(span.file_id, span.byte_offset, 2);
    let condition_span = Span::new(
        span.file_id,
        span.byte_offset,
        scrutinee.get_span().end() - span.byte_offset,
    );
    let replacement = format!("if {subject_text} == {pattern_text}");
    let mut diagnostic =
        diagnostics::lint::equatable_if_let(&if_keyword_span, pattern_text, subject_text);
    if !replacement_drops_comment(ctx.source, condition_span, &replacement) {
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(condition_span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
}

// Fielded variants and literals are excluded: an inner value can mismatch its
// field type without the outer pattern type showing it.
fn is_unit_variant_pattern(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::EnumVariant { identifier, fields, rest, .. }
        if !*rest && fields.is_empty() && identifier.contains('.'))
}
