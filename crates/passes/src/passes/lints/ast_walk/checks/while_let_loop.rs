use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};

use super::helpers::{enum_has_multiple_variants, span_text, unwrap_block};

pub fn check_while_let_loop(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Loop { body, ty, span, .. } = expression else {
        return;
    };

    // A `loop` carrying a value (`break v`) cannot become a statement-only
    // `while let`. A unit type means every exit is a plain `break`.
    if !ty.is_unit() {
        return;
    }

    let Expression::Block { items, .. } = body.as_ref() else {
        return;
    };
    let [
        Expression::Match {
            subject,
            arms,
            origin: MatchOrigin::Explicit,
            ..
        },
    ] = items.as_slice()
    else {
        return;
    };

    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return;
    }
    let (first, second) = (&arms[0], &arms[1]);

    // The second arm must be exactly `_ => break`, so the loop exits precisely
    // when the pattern fails to match, as a `while let` would.
    if !matches!(second.pattern, Pattern::WildCard { .. })
        || !matches!(
            unwrap_block(&second.expression),
            Expression::Break { value: None, .. }
        )
    {
        return;
    }

    // The first arm must be a refutable variant pattern; without another variant
    // to fall through to, the rewritten `while let` would loop forever.
    if !matches!(first.pattern, Pattern::EnumVariant { .. })
        || !enum_has_multiple_variants(&subject.get_type(), ctx.store)
    {
        return;
    }

    let pattern_span = first.pattern.get_span();
    let (Some(pattern_text), Some(subject_text)) = (
        ctx.source
            .get(pattern_span.byte_offset as usize..pattern_span.end() as usize),
        span_text(ctx.source, subject),
    ) else {
        return;
    };

    let loop_keyword = Span::new(span.file_id, span.byte_offset, 4);
    ctx.sink.push(diagnostics::lint::while_let_loop(
        &loop_keyword,
        pattern_text,
        subject_text,
    ));
}
