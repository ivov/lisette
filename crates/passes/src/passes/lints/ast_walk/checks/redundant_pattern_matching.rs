use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, MatchArm, Pattern, Span};
use syntax::types::unqualified_name;

use super::helpers::{as_tight_operand, bool_literal, span_text};

pub fn check_redundant_pattern_matching(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match {
        subject,
        arms,
        span,
        ..
    } = expression
    else {
        return;
    };

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

    // A bool newtype (`Flag(bool)`) result adapts the arms, which `.is_some()` loses.
    if !expression.get_type().is_boolean() {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    let mut diagnostic =
        diagnostics::lint::redundant_pattern_matching(&match_keyword_span, predicate);
    if let Some(subject_text) = span_text(ctx.source, subject) {
        let replacement = format!("{}.{predicate}()", as_tight_operand(subject_text, subject));
        diagnostic = diagnostic.with_fix(Fix::new(
            format!("Replace with `{replacement}`"),
            Edit::replacement(*span, replacement),
        ));
    }
    ctx.sink.push(diagnostic);
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
