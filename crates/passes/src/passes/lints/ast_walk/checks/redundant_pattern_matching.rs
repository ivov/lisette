use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};
use syntax::types::unqualified_name;

use super::helpers::bool_literal;

pub fn check_redundant_pattern_matching(expression: &Expression, ctx: &NodeCtx) {
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

    let subject_ty = subject.get_type();
    let is_option = subject_ty.is_option();
    let is_result = subject_ty.is_result();
    if !is_option && !is_result {
        return;
    }

    let (Some((first_variant, first_bool)), Some((second_variant, second_bool))) =
        (arm_variant_bool(&arms[0]), arm_variant_bool(&arms[1]))
    else {
        return;
    };

    if first_bool == second_bool {
        return;
    }

    let (true_variant, false_variant) = if first_bool {
        (first_variant, second_variant)
    } else {
        (second_variant, first_variant)
    };

    let predicate = match (is_option, true_variant, false_variant) {
        (true, "Some", "None") => "is_some",
        (true, "None", "Some") => "is_none",
        (false, "Ok", "Err") => "is_ok",
        (false, "Err", "Ok") => "is_err",
        _ => return,
    };

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink.push(diagnostics::lint::redundant_pattern_matching(
        &match_keyword_span,
        predicate,
    ));
}

fn arm_variant_bool(arm: &MatchArm) -> Option<(&str, bool)> {
    if arm.has_guard() {
        return None;
    }
    let Pattern::EnumVariant {
        identifier, fields, ..
    } = &arm.pattern
    else {
        return None;
    };
    if !fields
        .iter()
        .all(|field| matches!(field, Pattern::WildCard { .. }))
    {
        return None;
    }
    let value = bool_literal(arm.expression.unwrap_parens())?;
    Some((unqualified_name(identifier), value))
}
