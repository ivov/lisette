use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};
use syntax::types::{Type, unqualified_name};

use crate::passes::is_trivial_expression;
use semantics::store::Store;

pub fn check_match_as_if_let(expression: &Expression, ctx: &NodeCtx) {
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

    if matches!(origin, MatchOrigin::IfLet { .. }) {
        return;
    }

    if arms.len() != 2 {
        return;
    }

    let (first, second) = (&arms[0], &arms[1]);

    if first.has_guard() || second.has_guard() {
        return;
    }

    let (meaningful, dismissal) = if is_meaningful_arm(first) && is_trailing_dismissal(second) {
        (first, second)
    } else if is_meaningful_arm(second) && is_leading_dismissal(first, second) {
        (second, first)
    } else {
        return;
    };

    if !is_catchall(&dismissal.pattern) && enum_has_extra_variants(&subject.get_type(), ctx.store) {
        return;
    }

    let pattern_span = meaningful.pattern.get_span();
    let Some(pattern_text) = ctx
        .source
        .get(pattern_span.byte_offset as usize..pattern_span.end() as usize)
    else {
        return;
    };

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);

    ctx.sink.push(diagnostics::lint::match_as_if_let(
        &match_keyword_span,
        pattern_text,
    ));
}

fn is_meaningful_arm(arm: &MatchArm) -> bool {
    matches!(&arm.pattern, Pattern::EnumVariant { .. }) && !is_trivial_expression(&arm.expression)
}

fn is_catchall(pattern: &Pattern) -> bool {
    matches!(
        pattern,
        Pattern::WildCard { .. } | Pattern::Identifier { .. }
    )
}

fn is_trailing_dismissal(arm: &MatchArm) -> bool {
    let dismissable =
        is_catchall(&arm.pattern) || matches!(&arm.pattern, Pattern::EnumVariant { .. });
    dismissable && is_trivial_expression(&arm.expression)
}

fn is_leading_dismissal(dismissal: &MatchArm, meaningful: &MatchArm) -> bool {
    if !is_trivial_expression(&dismissal.expression) {
        return false;
    }
    match (
        variant_name(&dismissal.pattern),
        variant_name(&meaningful.pattern),
    ) {
        (Some(dismissed), Some(kept)) => dismissed != kept,
        _ => false,
    }
}

fn variant_name(pattern: &Pattern) -> Option<&str> {
    match pattern {
        Pattern::EnumVariant { identifier, .. } => Some(unqualified_name(identifier)),
        _ => None,
    }
}

fn enum_has_extra_variants(subject_ty: &Type, store: &Store) -> bool {
    let Type::Nominal { id, .. } = subject_ty.strip_refs() else {
        return false;
    };
    store
        .variants_of(&id)
        .is_some_and(|variants| variants.len() > 2)
}
