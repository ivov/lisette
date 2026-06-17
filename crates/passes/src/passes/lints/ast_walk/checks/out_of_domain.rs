use syntax::ast::{Expression, Literal, Span, UnaryOperator};
use syntax::program::CallKind;
use syntax::types::{SimpleKind, Type};

use crate::passes::walk::NodeCtx;
use semantics::store::{ClosedDomain, ClosedMember, DomainValue, Store};

/// Flags an out-of-domain literal targeting a `#[go(closed_domain)]` named
/// primitive, e.g. `payday(7)`, `time.Weekday(7)`, or `time.Weekday = -1` where
/// `time.Weekday` is closed over `0..=6`. Three trigger shapes:
///
/// - implicit adaptation: the literal carries the named type (the `7` in
///   `payday(7)` has type `time.Weekday`);
/// - negated implicit adaptation: `-1`, whose magnitude adapts but whose sign
///   lives on the parent `Unary` node;
/// - explicit construction: a newtype constructor call `time.Weekday(7)`.
///
/// The explicit `as` conversion (`7 as time.Weekday`) is the escape hatch and
/// does not warn.
///
/// `ctx.claimed_spans` collects the spans of magnitude literals owned by a parent
/// negation. The visitor walks parents before children, so the negation arm
/// claims its magnitude before the literal arm reaches it; this stops `-1` from
/// being judged on the magnitude `1` alone.
pub fn check_out_of_domain_value(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Literal { literal, ty, span } => {
            let Some(domain) = closed_domain_of(ty, ctx.store) else {
                return;
            };
            if ctx.claimed_spans.borrow().contains(span) {
                return;
            }
            let Some(value) = DomainValue::from_literal(literal, domain.base) else {
                return;
            };
            if !is_member(domain, &value) {
                emit(*span, domain, ctx);
            }
        }

        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression: inner,
            ty,
            span,
        } => {
            let Some(domain) = closed_domain_of(ty, ctx.store) else {
                return;
            };
            let Some((value, magnitude_span)) = negative_value(inner, domain.base) else {
                return;
            };
            ctx.claimed_spans.borrow_mut().insert(magnitude_span);
            if !is_member(domain, &value) {
                emit(*span, domain, ctx);
            }
        }

        Expression::Call {
            call_kind: Some(CallKind::TupleStructConstructor),
            args,
            ty,
            ..
        } => {
            let Some(domain) = closed_domain_of(ty, ctx.store) else {
                return;
            };
            let Some((value, span)) = args
                .first()
                .and_then(|arg| constructor_arg(arg, domain.base))
            else {
                return;
            };
            if !is_member(domain, &value) {
                emit(span, domain, ctx);
            }
        }

        _ => {}
    }
}

fn closed_domain_of<'a>(ty: &Type, store: &'a Store) -> Option<&'a ClosedDomain> {
    let Type::Nominal { id, .. } = ty else {
        return None;
    };
    store.closed_domains.get(id.as_str())
}

/// The negated value of a leading-minus integer literal, plus the magnitude
/// literal's span (claimed so the literal arm skips it).
fn negative_value(inner: &Expression, base: SimpleKind) -> Option<(DomainValue, Span)> {
    let Expression::Literal { literal, span, .. } = inner.unwrap_parens() else {
        return None;
    };
    Some((negate_literal(literal, base)?, *span))
}

/// A constructor argument keeps its base type, so it is read directly: a literal,
/// or a leading-minus literal (which carries no adaptation, hence no double-fire
/// to guard against).
fn constructor_arg(expression: &Expression, base: SimpleKind) -> Option<(DomainValue, Span)> {
    match expression.unwrap_parens() {
        Expression::Literal { literal, span, .. } => {
            Some((DomainValue::from_literal(literal, base)?, *span))
        }
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression: inner,
            span,
            ..
        } => {
            let Expression::Literal { literal, .. } = inner.unwrap_parens() else {
                return None;
            };
            Some((negate_literal(literal, base)?, *span))
        }
        _ => None,
    }
}

/// Negates a literal under its base kind. Defined for signed integer (and rune)
/// bases; unsigned bases reject negation (an inference error already).
fn negate_literal(literal: &Literal, base: SimpleKind) -> Option<DomainValue> {
    match literal {
        Literal::Integer { value, .. } if base.is_signed_int() => {
            Some(DomainValue::Int(-(*value as i128)))
        }
        _ => None,
    }
}

fn is_member(domain: &ClosedDomain, value: &DomainValue) -> bool {
    domain.members.iter().any(|member| member.value == *value)
}

fn emit(span: Span, domain: &ClosedDomain, ctx: &NodeCtx) {
    ctx.sink.push(diagnostics::lint::out_of_domain_value(
        &span,
        &domain.type_display,
        &render_valid(domain),
    ));
}

/// Renders a member as the user wrote it: runes keep their `'...'` surface form,
/// everything else renders its comparable value (already sign-corrected).
fn render_member(member: &ClosedMember) -> String {
    match (&member.literal, &member.value) {
        (Literal::Char(text), _) => format!("'{text}'"),
        (_, DomainValue::Int(value)) => value.to_string(),
        (_, DomainValue::Str(value)) => format!("\"{value}\""),
    }
}

/// Shows a contiguous integer domain as an inclusive range of its extremes;
/// every other domain (sparse, string, rune) lists its members, so the hint
/// never implies a gap value is valid.
fn render_valid(domain: &ClosedDomain) -> String {
    if domain.members.len() >= 2 && is_contiguous_integer_domain(domain) {
        let first = &domain.members[0];
        let last = &domain.members[domain.members.len() - 1];
        return format!(
            "{}={} ..= {}={}",
            first.display_name,
            render_member(first),
            last.display_name,
            render_member(last),
        );
    }

    domain
        .members
        .iter()
        .map(|member| format!("{}={}", member.display_name, render_member(member)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Members are pre-sorted by value; the domain is contiguous when every adjacent
/// integer differs by one. Non-integer (string, escaped rune) members are not a
/// range.
fn is_contiguous_integer_domain(domain: &ClosedDomain) -> bool {
    let mut previous: Option<i128> = None;
    for member in &domain.members {
        let DomainValue::Int(value) = member.value else {
            return false;
        };
        if previous.is_some_and(|p| value != p + 1) {
            return false;
        }
        previous = Some(value);
    }
    previous.is_some()
}
