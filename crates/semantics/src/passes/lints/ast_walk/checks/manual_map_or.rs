use crate::passes::walk::{NodeCtx, visit_ast};
use syntax::ast::{Expression, MatchArm, MatchOrigin, Pattern, RestPattern, Span};
use syntax::types::unqualified_name;

use super::helpers::{enum_variant_binding, has_escaping_control_flow, is_eager_safe};

pub fn check_manual_map_or(expression: &Expression, ctx: &NodeCtx) {
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

    if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
        return;
    }

    let subject_ty = subject.get_type();
    let result_ty = expression.get_type();
    if result_ty.is_unit() || result_ty.is_option() || result_ty.is_result() {
        return;
    }
    let (success_variant, failure_variant) = if subject_ty.is_option() {
        ("Some", "None")
    } else if subject_ty.is_result() {
        ("Ok", "Err")
    } else {
        return;
    };

    let (success_arm, default) = if is_mapped_success_arm(&arms[0], success_variant) {
        (&arms[0], failure_default(&arms[1], failure_variant))
    } else if is_mapped_success_arm(&arms[1], success_variant) {
        (&arms[1], failure_default(&arms[0], failure_variant))
    } else {
        return;
    };
    let Some(default) = default else {
        return;
    };

    if produces_no_value(&success_arm.expression) || produces_no_value(default) {
        return;
    }

    if default.diverges().is_some() {
        return;
    }
    // A non-trivial default keeps the lazy form, so eager `map_or` never surfaces a
    // panic (e.g. `1 / denom`) on the success path.
    let lazy_default = !is_eager_safe(default);
    if lazy_default && has_escaping_control_flow(default) {
        return;
    }

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    ctx.sink.push(diagnostics::lint::manual_map_or(
        &match_keyword_span,
        lazy_default,
    ));
}

fn produces_no_value(expression: &Expression) -> bool {
    let ty = expression.get_type();
    ty.is_unit() || ty.is_ignored()
}

// A `Some(v)`/`Ok(v)` arm whose body transforms the bound value, rather than
// returning it bare (which `manual_unwrap_or` covers).
fn is_mapped_success_arm(arm: &MatchArm, success_variant: &str) -> bool {
    let Some(binding) = enum_variant_binding(&arm.pattern, success_variant) else {
        return false;
    };
    let mapped = unwrap_trivial(&arm.expression);
    if matches!(mapped, Expression::Identifier { value, .. } if value.as_str() == binding) {
        return false;
    }
    maps_binding(mapped, binding) && !has_escaping_control_flow(mapped)
}

fn unwrap_trivial(expression: &Expression) -> &Expression {
    match expression.unwrap_parens() {
        Expression::Block { items, .. } if items.len() == 1 => unwrap_trivial(&items[0]),
        other => other,
    }
}

fn failure_default<'a>(arm: &'a MatchArm, failure_variant: &str) -> Option<&'a Expression> {
    let Pattern::EnumVariant {
        identifier, fields, ..
    } = &arm.pattern
    else {
        return None;
    };
    if unqualified_name(identifier) != failure_variant {
        return None;
    }
    if !fields
        .iter()
        .all(|field| matches!(field, Pattern::WildCard { .. }))
    {
        return None;
    }
    Some(arm.expression.unwrap_parens())
}

// Whether `body` maps the bound value: it must reference the binding and must not
// shadow it with an inner binding of the same name (whose reference is to the
// shadow, not the matched value).
fn maps_binding(body: &Expression, binding: &str) -> bool {
    let mut referenced = false;
    let mut shadowed = false;
    visit_ast(
        std::slice::from_ref(body),
        &mut |node| {
            if matches!(node, Expression::Identifier { value, .. } if value.as_str() == binding) {
                referenced = true;
            }
        },
        &mut |pattern| {
            shadowed |= pattern_binds(pattern, binding);
        },
    );
    referenced && !shadowed
}

fn pattern_binds(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Identifier { identifier, .. } => identifier.as_str() == name,
        Pattern::AsBinding { name: bound, .. } => bound.as_str() == name,
        Pattern::Slice {
            rest: RestPattern::Bind { name: bound, .. },
            ..
        } => bound.as_str() == name,
        _ => false,
    }
}
