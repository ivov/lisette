use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Literal, MatchArm, Pattern};

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

    ctx.sink.push(diagnostics::lint::redundant_guards(
        span,
        binding,
        literal_text,
    ));
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
