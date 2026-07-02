use crate::passes::lints::span_edit::match_arm_deletion;
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use ecow::EcoString;
use syntax::ast::{Expression, Literal, MatchArm, TypedPattern, collect_pattern_bindings};
use syntax::types::unqualified_name;

use super::helpers::{expressions_equivalent, is_empty_block};

pub fn check_match_same_arms(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match { arms, .. } = expression else {
        return;
    };

    if arms.len() < 3 {
        return;
    }

    // The all-arms-identical case is owned by `identical_match_arms`.
    let first = &arms[0].expression;
    if arms
        .iter()
        .all(|arm| expressions_equivalent(first, &arm.expression))
    {
        return;
    }

    for (index, later) in arms.iter().enumerate().skip(1) {
        if !is_mergeable(later) {
            continue;
        }
        let Some(later_pattern) = later.typed_pattern.as_ref() else {
            continue;
        };
        // Each arm between, and the earlier arm itself, must provably not match the
        // later value, or the merge reroutes it. Guards are opaque here, so an
        // overlapping guarded arm still blocks the merge.
        let earlier = arms[..index]
            .iter()
            .enumerate()
            .find_map(|(earlier_index, earlier)| {
                let safe = is_mergeable(earlier)
                    && expressions_equivalent(&earlier.expression, &later.expression)
                    && disjoint_from_later(earlier, later_pattern)
                    && arms[earlier_index + 1..index]
                        .iter()
                        .all(|between| disjoint_from_later(between, later_pattern));
                safe.then_some(earlier)
            });
        let Some(earlier) = earlier else {
            continue;
        };
        let earlier_span = earlier.pattern.get_span();
        let later_span = later.pattern.get_span();
        let (Some(earlier_text), Some(later_text)) = (
            ctx.source
                .get(earlier_span.byte_offset as usize..earlier_span.end() as usize),
            ctx.source
                .get(later_span.byte_offset as usize..later_span.end() as usize),
        ) else {
            continue;
        };

        let merged = format!("{earlier_text} | {later_text}");
        let arm_span = later_span.merge(later.expression.get_span());
        let deletion = match_arm_deletion(ctx.source, arm_span);
        ctx.sink.push(
            diagnostics::lint::match_same_arms(&later_span, earlier_text).with_fix(Fix::multi(
                format!("Merge into `{merged}`"),
                vec![
                    Edit::replacement(earlier_span, merged),
                    Edit::deletion(deletion),
                ],
            )),
        );
    }
}

fn is_mergeable(arm: &MatchArm) -> bool {
    !arm.has_guard()
        && !is_empty_block(&arm.expression)
        // On the surface pattern: the typed form erases an `as` binding, and only a
        // binding-free value arm can join an `|` merge.
        && collect_pattern_bindings(&arm.pattern).is_empty()
        && arm.typed_pattern.as_ref().is_some_and(is_singleton_typed)
}

fn disjoint_from_later(arm: &MatchArm, later_pattern: &TypedPattern) -> bool {
    arm.typed_pattern
        .as_ref()
        .is_some_and(|pattern| typed_patterns_disjoint(pattern, later_pattern))
}

fn is_singleton_typed(pattern: &TypedPattern) -> bool {
    match pattern {
        TypedPattern::Literal(_) | TypedPattern::Const { .. } => true,
        TypedPattern::EnumVariant { fields, .. } => fields.iter().all(is_singleton_typed),
        TypedPattern::EnumStructVariant {
            variant_fields,
            pattern_fields,
            ..
        } => {
            pattern_fields.len() == variant_fields.len()
                && pattern_fields.iter().all(|(_, p)| is_singleton_typed(p))
        }
        TypedPattern::Struct {
            struct_fields,
            pattern_fields,
            ..
        } => {
            pattern_fields.len() == struct_fields.len()
                && pattern_fields.iter().all(|(_, p)| is_singleton_typed(p))
        }
        TypedPattern::Tuple { elements, .. } => elements.iter().all(is_singleton_typed),
        TypedPattern::Wildcard | TypedPattern::Slice { .. } | TypedPattern::Or { .. } => false,
    }
}

// Conservative: `false` unless disjointness is proven. A const is a value
// comparison (compare folded values, never names). An enum variant is a
// constructor (distinct names are disjoint).
fn typed_patterns_disjoint(a: &TypedPattern, b: &TypedPattern) -> bool {
    use TypedPattern as T;
    match (a, b) {
        (T::Literal(la), T::Literal(lb)) => distinct_literals(la, lb),
        (
            T::Const {
                value: Some(la), ..
            },
            T::Const {
                value: Some(lb), ..
            },
        ) => distinct_literals(la, lb),
        (
            T::Const {
                value: Some(cv), ..
            },
            T::Literal(lv),
        )
        | (
            T::Literal(lv),
            T::Const {
                value: Some(cv), ..
            },
        ) => distinct_literals(cv, lv),
        (
            T::EnumVariant {
                enum_name: ea,
                variant_name: va,
                fields: fa,
                ..
            },
            T::EnumVariant {
                enum_name: eb,
                variant_name: vb,
                fields: fb,
                ..
            },
        ) => {
            // `variant_name` keeps its raw spelling, so a qualified `Sig.A` and a
            // bare `A` differ as strings. Compare resolved enum + unqualified name.
            ea == eb
                && (unqualified_name(va) != unqualified_name(vb)
                    || (fa.len() == fb.len()
                        && fa
                            .iter()
                            .zip(fb)
                            .any(|(x, y)| typed_patterns_disjoint(x, y))))
        }
        (
            T::EnumStructVariant {
                enum_name: ea,
                variant_name: va,
                pattern_fields: fa,
                ..
            },
            T::EnumStructVariant {
                enum_name: eb,
                variant_name: vb,
                pattern_fields: fb,
                ..
            },
        ) => {
            ea == eb
                && (unqualified_name(va) != unqualified_name(vb) || struct_fields_disjoint(fa, fb))
        }
        (
            T::Struct {
                pattern_fields: fa, ..
            },
            T::Struct {
                pattern_fields: fb, ..
            },
        ) => struct_fields_disjoint(fa, fb),
        (T::Tuple { elements: ea, .. }, T::Tuple { elements: eb, .. }) => {
            ea.len() == eb.len()
                && ea
                    .iter()
                    .zip(eb)
                    .any(|(x, y)| typed_patterns_disjoint(x, y))
        }
        _ => false,
    }
}

fn struct_fields_disjoint(
    a: &[(EcoString, TypedPattern)],
    b: &[(EcoString, TypedPattern)],
) -> bool {
    a.iter().any(|(name, ap)| {
        b.iter()
            .find(|(other, _)| other == name)
            .is_some_and(|(_, bp)| typed_patterns_disjoint(ap, bp))
    })
}

// Floats are excluded (NaN != NaN). Non-scalar literals are not compared.
fn distinct_literals(a: &Literal, b: &Literal) -> bool {
    match (a, b) {
        (Literal::Integer { value: x, .. }, Literal::Integer { value: y, .. }) => x != y,
        (Literal::Boolean(x), Literal::Boolean(y)) => x != y,
        (Literal::String { value: x, .. }, Literal::String { value: y, .. }) => x != y,
        (Literal::Char(x), Literal::Char(y)) => x != y,
        _ => false,
    }
}
