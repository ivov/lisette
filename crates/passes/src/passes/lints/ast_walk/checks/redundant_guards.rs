use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{BinaryOperator, Expression, Literal, MatchArm, Pattern, Span};

use syntax::ast::collect_pattern_bindings;

use super::helpers::{is_float_operand, mentions_identifier, span_text};

pub fn check_redundant_guards(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match { arms, .. } = expression else {
        return;
    };

    for arm in arms {
        check_arm(arm, ctx);
    }
}

fn check_arm(arm: &MatchArm, ctx: &NodeCtx) {
    let Some(guard) = &arm.guard else {
        return;
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
        span,
        ..
    } = guard.unwrap_parens()
    else {
        return;
    };

    // Never fold a checker-rejected equality into the pattern.
    if ctx.facts.type_error_spans.contains(span) {
        return;
    }

    let (binding_expression, binding, literal) = match (binding_name(left), foldable_literal(right))
    {
        (Some(name), true) => (left, name, right),
        _ => match (binding_name(right), foldable_literal(left)) {
            (Some(name), true) => (right, name, left),
            _ => return,
        },
    };

    // Float bindings are out of scope: a `Some(1.0)` pattern is dicey around NaN.
    if is_float_operand(ctx.store, binding_expression) {
        return;
    }

    // Bound exactly once, as a plain identifier, so the literal swap is unambiguous.
    let bindings = collect_pattern_bindings(&arm.pattern);
    if bindings.iter().filter(|(name, _)| name == binding).count() != 1
        || !binds_as_identifier(&arm.pattern, binding)
    {
        return;
    }

    if mentions_identifier(&arm.expression, binding) {
        return;
    }

    let Some(literal_text) = span_text(ctx.source, literal) else {
        return;
    };

    let mut diagnostic = diagnostics::lint::redundant_guards(span, binding, literal_text);
    if let Some((_, binding_span)) = bindings.iter().find(|(name, _)| name == binding)
        && !binding_in_struct(&arm.pattern, binding)
    {
        let pattern_end = arm.pattern.get_span().end();
        let guard_end = guard.get_span().end();
        let guard_deletion = Span::new(span.file_id, pattern_end, guard_end - pattern_end);
        diagnostic = diagnostic.with_fix(Fix::multi(
            format!("Fold `{binding} == {literal_text}` into the pattern"),
            vec![
                Edit::replacement(*binding_span, literal_text.to_string()),
                Edit::deletion(guard_deletion),
            ],
        ));
    }
    ctx.sink.push(diagnostic);
}

fn binding_name(expression: &Expression) -> Option<&str> {
    match expression.unwrap_parens() {
        Expression::Identifier { value, .. } => Some(value.as_str()),
        _ => None,
    }
}

// Floats are excluded (NaN). Bools are owned by `bool_literal_comparison`.
fn foldable_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Integer { .. } | Literal::String { .. },
            ..
        }
    )
}

// A struct-bound `x` would swap to invalid `{ 5 }`, not `{ x: 5 }`, so skip it.
fn binding_in_struct(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Struct { fields, .. } => fields
            .iter()
            .any(|field| binds_as_identifier(&field.value, name)),
        Pattern::EnumVariant { fields, .. }
        | Pattern::Tuple {
            elements: fields, ..
        } => fields.iter().any(|field| binding_in_struct(field, name)),
        Pattern::Slice { prefix, .. } => prefix.iter().any(|p| binding_in_struct(p, name)),
        Pattern::AsBinding { pattern, .. } => binding_in_struct(pattern, name),
        _ => false,
    }
}

fn binds_as_identifier(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Identifier { identifier, .. } => identifier == name,
        Pattern::EnumVariant { fields, .. }
        | Pattern::Tuple {
            elements: fields, ..
        } => fields.iter().any(|field| binds_as_identifier(field, name)),
        Pattern::Struct { fields, .. } => fields
            .iter()
            .any(|field| binds_as_identifier(&field.value, name)),
        Pattern::Slice { prefix, .. } => prefix.iter().any(|p| binds_as_identifier(p, name)),
        Pattern::AsBinding { pattern, .. } => binds_as_identifier(pattern, name),
        _ => false,
    }
}
