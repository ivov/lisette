use crate::passes::lints::span_edit::match_arm_deletion;
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use semantics::store::Store;
use syntax::ast::{
    ConstructorPatternResolution, Expression, Literal, MatchArm, Pattern, RecordPatternResolution,
    StructFieldPattern, collect_pattern_bindings,
};
use syntax::program::DefinitionBody;
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
        if !is_mergeable(later, ctx.store) {
            continue;
        }
        let later_pattern = &later.pattern;
        // Each arm between, and the earlier arm itself, must provably not match the
        // later value, or the merge reroutes it. Guards are opaque here, so an
        // overlapping guarded arm still blocks the merge.
        let earlier = arms[..index]
            .iter()
            .enumerate()
            .find_map(|(earlier_index, earlier)| {
                let safe = is_mergeable(earlier, ctx.store)
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

fn is_mergeable(arm: &MatchArm, store: &Store) -> bool {
    !arm.has_guard()
        && !is_empty_block(&arm.expression)
        // Only a binding-free value arm can join an `|` merge.
        && collect_pattern_bindings(&arm.pattern).is_empty()
        && is_singleton_pattern(&arm.pattern, store)
}

fn disjoint_from_later(arm: &MatchArm, later_pattern: &Pattern) -> bool {
    patterns_disjoint(&arm.pattern, later_pattern)
}

fn is_singleton_pattern(pattern: &Pattern, store: &Store) -> bool {
    match pattern {
        Pattern::Literal { .. } => true,
        Pattern::EnumVariant {
            resolution: ConstructorPatternResolution::Const { .. },
            ..
        } => true,
        Pattern::EnumVariant {
            fields, resolution, ..
        } => constructor_arity(resolution, store).is_some_and(|arity| {
            fields.len() == arity
                && fields
                    .iter()
                    .all(|field| is_singleton_pattern(field, store))
        }),
        Pattern::Struct {
            fields, resolution, ..
        } => record_arity(resolution, store).is_some_and(|arity| {
            fields.len() == arity
                && fields
                    .iter()
                    .all(|field| is_singleton_pattern(&field.value, store))
        }),
        Pattern::Tuple { elements, .. } => elements
            .iter()
            .all(|element| is_singleton_pattern(element, store)),
        Pattern::AsBinding { pattern, .. } => is_singleton_pattern(pattern, store),
        Pattern::Unit { .. }
        | Pattern::Identifier { .. }
        | Pattern::WildCard { .. }
        | Pattern::Slice { .. }
        | Pattern::Or { .. } => false,
    }
}

fn constructor_arity(resolution: &ConstructorPatternResolution, store: &Store) -> Option<usize> {
    let ConstructorPatternResolution::EnumVariant {
        enum_name,
        variant_name,
    } = resolution
    else {
        return None;
    };
    match &store.get_definition(enum_name)?.body {
        DefinitionBody::Struct { fields, .. } => Some(fields.len()),
        DefinitionBody::Enum { variants, .. } => variants
            .iter()
            .find(|variant| variant.name == unqualified_name(variant_name))
            .map(|variant| variant.fields.len()),
        _ => None,
    }
}

fn record_arity(resolution: &RecordPatternResolution, store: &Store) -> Option<usize> {
    match resolution {
        RecordPatternResolution::Struct { struct_name } => {
            match &store.get_definition(struct_name)?.body {
                DefinitionBody::Struct { fields, .. } => Some(fields.len()),
                _ => None,
            }
        }
        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
        } => match &store.get_definition(enum_name)?.body {
            DefinitionBody::Enum { variants, .. } => variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))
                .map(|variant| variant.fields.len()),
            _ => None,
        },
        RecordPatternResolution::Unresolved => None,
    }
}

// Conservative: `false` unless disjointness is proven. A const is a value
// comparison (compare folded values, never names). An enum variant is a
// constructor (distinct names are disjoint).
fn patterns_disjoint(a: &Pattern, b: &Pattern) -> bool {
    use Pattern as P;
    match (a, b) {
        (P::AsBinding { pattern, .. }, other) | (other, P::AsBinding { pattern, .. }) => {
            patterns_disjoint(pattern, other)
        }
        (P::Literal { literal: la, .. }, P::Literal { literal: lb, .. }) => {
            distinct_literals(la, lb)
        }
        (
            P::EnumVariant {
                resolution:
                    ConstructorPatternResolution::Const {
                        value: Some(la), ..
                    },
                ..
            },
            P::EnumVariant {
                resolution:
                    ConstructorPatternResolution::Const {
                        value: Some(lb), ..
                    },
                ..
            },
        ) => distinct_literals(la, lb),
        (
            P::EnumVariant {
                resolution:
                    ConstructorPatternResolution::Const {
                        value: Some(cv), ..
                    },
                ..
            },
            P::Literal { literal: lv, .. },
        )
        | (
            P::Literal { literal: lv, .. },
            P::EnumVariant {
                resolution:
                    ConstructorPatternResolution::Const {
                        value: Some(cv), ..
                    },
                ..
            },
        ) => distinct_literals(cv, lv),
        (
            P::EnumVariant {
                fields: fa,
                resolution:
                    ConstructorPatternResolution::EnumVariant {
                        enum_name: ea,
                        variant_name: va,
                        ..
                    },
                ..
            },
            P::EnumVariant {
                fields: fb,
                resolution:
                    ConstructorPatternResolution::EnumVariant {
                        enum_name: eb,
                        variant_name: vb,
                        ..
                    },
                ..
            },
        ) => {
            // `variant_name` keeps its raw spelling, so a qualified `Sig.A` and a
            // bare `A` differ as strings. Compare resolved enum + unqualified name.
            ea == eb
                && (unqualified_name(va) != unqualified_name(vb)
                    || (fa.len() == fb.len()
                        && fa.iter().zip(fb).any(|(x, y)| patterns_disjoint(x, y))))
        }
        (
            P::Struct {
                fields: fa,
                resolution:
                    RecordPatternResolution::EnumVariant {
                        enum_name: ea,
                        variant_name: va,
                        ..
                    },
                ..
            },
            P::Struct {
                fields: fb,
                resolution:
                    RecordPatternResolution::EnumVariant {
                        enum_name: eb,
                        variant_name: vb,
                        ..
                    },
                ..
            },
        ) => {
            ea == eb
                && (unqualified_name(va) != unqualified_name(vb) || struct_fields_disjoint(fa, fb))
        }
        (
            P::Struct {
                fields: fa,
                resolution: RecordPatternResolution::Struct { .. },
                ..
            },
            P::Struct {
                fields: fb,
                resolution: RecordPatternResolution::Struct { .. },
                ..
            },
        ) => struct_fields_disjoint(fa, fb),
        (P::Tuple { elements: ea, .. }, P::Tuple { elements: eb, .. }) => {
            ea.len() == eb.len() && ea.iter().zip(eb).any(|(x, y)| patterns_disjoint(x, y))
        }
        _ => false,
    }
}

fn struct_fields_disjoint(a: &[StructFieldPattern], b: &[StructFieldPattern]) -> bool {
    a.iter().any(|field| {
        b.iter()
            .find(|other| other.name == field.name)
            .is_some_and(|other| patterns_disjoint(&field.value, &other.value))
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
