use diagnostics::LisetteDiagnostic;
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, Span};
use syntax::types::unqualified_name;

use super::helpers::{has_escaping_control_flow, is_side_effect_free};

pub fn check_manual_unwrap_or(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
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
    let (success_variant, failure_variant) = if subject_ty.is_option() {
        ("Some", "None")
    } else if subject_ty.is_result() {
        ("Ok", "Err")
    } else {
        return;
    };

    let default = if is_success_arm(&arms[0], success_variant) {
        failure_default(&arms[1], failure_variant)
    } else if is_success_arm(&arms[1], success_variant) {
        failure_default(&arms[0], failure_variant)
    } else {
        return;
    };

    let Some(default) = default else {
        return;
    };

    if default.diverges().is_some() {
        return;
    }

    let default_has_effects = !is_side_effect_free(default);
    if default_has_effects && has_escaping_control_flow(default) {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    diagnostics.push(diagnostics::lint::manual_unwrap_or(
        &match_keyword_span,
        default_has_effects,
    ));
}

fn enum_variant_fields<'a>(arm: &'a MatchArm, variant: &str) -> Option<&'a [Pattern]> {
    if arm.has_guard() {
        return None;
    }
    let Pattern::EnumVariant {
        identifier, fields, ..
    } = &arm.pattern
    else {
        return None;
    };
    if unqualified_name(identifier) != variant {
        return None;
    }
    Some(fields)
}

fn is_success_arm(arm: &MatchArm, success_variant: &str) -> bool {
    let Some(fields) = enum_variant_fields(arm, success_variant) else {
        return false;
    };
    let [
        Pattern::Identifier {
            identifier: bound, ..
        },
    ] = fields
    else {
        return false;
    };
    let Expression::Identifier { value, .. } = arm.expression.unwrap_parens() else {
        return false;
    };
    value == bound
}

fn failure_default<'a>(arm: &'a MatchArm, failure_variant: &str) -> Option<&'a Expression> {
    let fields = enum_variant_fields(arm, failure_variant)?;
    if !fields
        .iter()
        .all(|field| matches!(field, Pattern::WildCard { .. }))
    {
        return None;
    }
    Some(arm.expression.unwrap_parens())
}
