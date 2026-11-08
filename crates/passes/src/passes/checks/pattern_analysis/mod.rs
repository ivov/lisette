mod escape;
mod inhabitance;
mod maranget;
mod normalize;
mod pattern_matrix;
mod types;
mod witness;

use crate::passes::is_trivial_expression;

pub use inhabitance::InhabitanceCache;
pub use inhabitance::is_inhabited;
pub use maranget::check_exhaustiveness;
pub use normalize::{NormalizationContext, normalize_pattern};
pub use types::*;
pub use witness::format_witness;

pub use self::PatternAnalysisContext as Context;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{IssueKind, LocalSink};
use semantics::store::Store;
use std::slice;
use syntax::ast::MatchArm;
use syntax::ast::{
    ConstructorPatternResolution, Expression, IfLetAlternative, Literal, Pattern, SelectArm, Span,
};
use syntax::types::{Type, unqualified_name};

use maranget::is_useful;
use normalize::normalize_arm;

pub struct PatternAnalysisContext<'a, 'sink> {
    pub store: &'a Store,
    cache: InhabitanceCache,
    pattern_error_spans: &'a HashSet<Span>,
    sink: &'sink LocalSink,
    lint_sink: Option<&'sink LocalSink>,
}

impl<'a, 'sink> PatternAnalysisContext<'a, 'sink> {
    pub fn new(
        store: &'a Store,
        pattern_error_spans: &'a HashSet<Span>,
        sink: &'sink LocalSink,
        lint_sink: Option<&'sink LocalSink>,
    ) -> Self {
        Self {
            store,
            cache: InhabitanceCache::new(),
            pattern_error_spans,
            sink,
            lint_sink,
        }
    }

    pub fn sink(&self) -> &'sink LocalSink {
        self.sink
    }

    fn normalize_context(&self) -> NormalizationContext<'a> {
        NormalizationContext {
            store: self.store,
            scrutinee_type: None,
        }
    }

    fn normalize_context_for_match(&self, scrutinee_type: Type) -> NormalizationContext<'a> {
        NormalizationContext {
            store: self.store,
            scrutinee_type: Some(scrutinee_type),
        }
    }

    fn add_issue(&self, span: Span, kind: IssueKind) {
        if let Some(sink) = self.lint_sink {
            sink.push(diagnostics::lint::pattern_issue(&span, kind));
        }
    }
}

pub fn check(expression: &Expression, ctx: &mut PatternAnalysisContext) {
    let sink = ctx.sink;
    match expression {
        Expression::Literal { literal, .. } => {
            if let Literal::Slice(expressions) = literal {
                for e in expressions {
                    check(e, ctx);
                }
            }
        }

        Expression::Function { params, body, .. } => {
            for param in params {
                if !check_refutability(&param.pattern, ctx) {
                    return;
                }
            }
            if let Some(body) = body.definition() {
                check(body, ctx);
            }
        }
        Expression::Lambda { params, body, .. } => {
            for param in params {
                if !check_refutability(&param.pattern, ctx) {
                    return;
                }
            }
            check(body, ctx);
        }

        Expression::Let {
            binding,
            value,
            mode,
            ..
        } => {
            check(value, ctx);

            if is_rejected(value) || has_rejected_pattern(&binding.pattern, ctx.pattern_error_spans)
            {
                if let Some(else_expression) = mode.else_block() {
                    check(else_expression, ctx);
                }
            } else if let Some(else_expression) = mode.else_block() {
                check(else_expression, ctx);

                if is_pattern_irrefutable(&binding.pattern, ctx.store) {
                    ctx.add_issue(binding.pattern.get_span(), IssueKind::RedundantLetElse);
                }
            } else if mode.is_assert() {
                if is_pattern_irrefutable(&binding.pattern, ctx.store) {
                    ctx.add_issue(binding.pattern.get_span(), IssueKind::RedundantLetAssert);
                }
            } else {
                check_refutability(&binding.pattern, ctx);
            }
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            ..
        } => {
            check(scrutinee, ctx);
            if !is_rejected(scrutinee) && !has_rejected_pattern(pattern, ctx.pattern_error_spans) {
                check_if_let(pattern, alternative, ctx);
            }
            check(consequence, ctx);
            if let Some(alternative) = alternative.expression() {
                check(alternative, ctx);
            }
        }

        Expression::Match {
            subject,
            arms,
            span,
            ..
        } => {
            check(subject, ctx);

            if is_rejected(subject) {
                for a in arms {
                    check(&a.expression, ctx);
                    if let Some(guard) = &a.guard {
                        check(guard, ctx);
                    }
                }
                return;
            }

            if !is_inhabited(&subject.get_type(), ctx.store, &mut ctx.cache) {
                return;
            }

            let mut unions = HashMap::default();
            let norm_ctx = ctx.normalize_context_for_match(subject.get_type());

            let rejected_spans = ctx.pattern_error_spans;
            if arms
                .iter()
                .any(|arm| has_rejected_pattern(&arm.pattern, rejected_spans))
            {
                check_redundancy_with_guards(
                    arms,
                    rejected_spans,
                    &mut unions,
                    &norm_ctx,
                    &mut ctx.cache,
                    sink,
                );
                for a in arms {
                    check(&a.expression, ctx);
                    if let Some(guard) = &a.guard {
                        check(guard, ctx);
                    }
                }
                return;
            }

            let unguarded_rows: Vec<Row> = arms
                .iter()
                .filter(|arm| !arm.has_guard())
                .flat_map(|arm| normalize_arm(arm, &mut unions, &norm_ctx, &mut ctx.cache))
                .collect();

            if let Err(witnesses) = check_exhaustiveness(&unguarded_rows, &unions) {
                let mut cases: Vec<String> = witnesses.iter().map(format_witness).collect();
                cases.sort();
                cases.dedup();

                let subject_span = subject.get_span();
                let match_span = Span::new(
                    span.file_id,
                    span.byte_offset,
                    (subject_span.byte_offset + subject_span.byte_length) - span.byte_offset,
                );

                sink.push(diagnostics::pattern::non_exhaustive(match_span, &cases));
                return;
            }

            if !check_redundancy_with_guards(
                arms,
                rejected_spans,
                &mut unions,
                &norm_ctx,
                &mut ctx.cache,
                sink,
            ) {
                return;
            }

            for a in arms {
                check(&a.expression, ctx);
                if let Some(guard) = &a.guard {
                    check(guard, ctx);
                }
            }
        }

        Expression::StructCall { spread, .. } => {
            if let Some(expression) = spread.as_expression() {
                check(expression, ctx);
            }
        }
        Expression::Assignment { .. } => {}

        Expression::Interface { .. } => {}

        Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } => {
            check(scrutinee, ctx);
            check(body, ctx);

            if !is_rejected(scrutinee)
                && !has_rejected_pattern(pattern, ctx.pattern_error_spans)
                && is_pattern_irrefutable(pattern, ctx.store)
            {
                sink.push(diagnostics::pattern::irrefutable_while_let(
                    pattern.get_span(),
                ));
            }
        }

        Expression::For {
            binding,
            iterable,
            body,
            ..
        } => {
            if !check_refutability(&binding.pattern, ctx) {
                return;
            }
            check(iterable, ctx);
            check(body, ctx);
        }

        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive {
                        receive_expression,
                        body,
                        ..
                    } => {
                        check(receive_expression.as_ref(), ctx);
                        check(body.as_ref(), ctx);
                    }
                    SelectArm::Send {
                        send_expression,
                        body,
                    } => {
                        check(send_expression.as_ref(), ctx);
                        check(body.as_ref(), ctx);
                    }
                    SelectArm::MatchReceive {
                        receive_expression,
                        arms: match_arms,
                    } => {
                        check(receive_expression.as_ref(), ctx);
                        for match_arm in match_arms {
                            check(&match_arm.expression, ctx);
                        }
                    }
                    SelectArm::WildCard { body } => {
                        check(body.as_ref(), ctx);
                    }
                }
            }
        }
        _ => {
            for child in expression.children() {
                check(child, ctx);
            }
        }
    }
}

fn check_redundancy_with_guards(
    arms: &[MatchArm],
    rejected_spans: &HashSet<Span>,
    unions: &mut UnionTable,
    norm_ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
    sink: &LocalSink,
) -> bool {
    let mut unguarded_previous: Vec<(usize, Row)> = vec![];
    let mut found_redundant = false;

    for (index, arm) in arms.iter().enumerate() {
        if rejected_spans.contains(&arm.pattern.get_span()) {
            continue;
        }
        let alternatives: Vec<&Pattern> = match &arm.pattern {
            Pattern::Or { patterns, .. } => patterns.iter().collect(),
            pattern => vec![pattern],
        };
        let valid_alternatives: Vec<&Pattern> = alternatives
            .iter()
            .copied()
            .filter(|pattern| !has_rejected_pattern(pattern, rejected_spans))
            .collect();
        let has_rejected_alternative = valid_alternatives.len() < alternatives.len();
        let current_rows: Vec<Row> = valid_alternatives
            .iter()
            .map(|pattern| vec![normalize_pattern(pattern, unions, norm_ctx, cache)])
            .collect();

        let mut current_arm_rows: Vec<Row> = vec![];
        let mut redundant_alternatives = Vec::new();

        for (alt_index, current_row) in current_rows.iter().enumerate() {
            let mut previous_rows: Vec<Row> =
                unguarded_previous.iter().map(|(_, r)| r.clone()).collect();
            previous_rows.extend(current_arm_rows.iter().cloned());

            if !is_useful(&previous_rows, current_row, unions) {
                let span = if matches!(arm.pattern, Pattern::Or { .. }) {
                    valid_alternatives[alt_index].get_span()
                } else {
                    arm.pattern.get_span()
                };

                let covered_by_same_arm = current_arm_rows
                    .iter()
                    .any(|prev| !is_useful(slice::from_ref(prev), current_row, unions));

                let help = if covered_by_same_arm {
                    "This alternative is unreachable because it is already covered by an earlier alternative in the same arm"
                        .to_string()
                } else {
                    let covering = unguarded_previous
                        .iter()
                        .find(|(_, prev)| !is_useful(slice::from_ref(prev), current_row, unions))
                        .map(|(orig_idx, prev)| (*orig_idx, prev));

                    if let Some((covering_index, covering_row)) = covering {
                        let covering_pattern = covering_row
                            .first()
                            .map(witness::format_pattern)
                            .unwrap_or_default();
                        format!(
                            "This pattern is unreachable because it is already covered by arm #{}: `{}`",
                            covering_index + 1,
                            covering_pattern
                        )
                    } else {
                        "This pattern is covered by earlier match arms and will never be reached"
                            .to_string()
                    }
                };

                let label = if covered_by_same_arm {
                    "this alternative is unreachable".to_string()
                } else {
                    format!("arm #{} is unreachable", index + 1)
                };

                redundant_alternatives.push((span, label, help, covered_by_same_arm));
                found_redundant = true;
            }

            current_arm_rows.push(current_row.clone());
        }

        let whole_arm_redundant = !has_rejected_alternative
            && current_rows.len() > 1
            && redundant_alternatives.len() == current_rows.len()
            && redundant_alternatives
                .iter()
                .all(|(_, _, _, covered_by_same_arm)| !covered_by_same_arm);
        if whole_arm_redundant {
            let (_, label, help, _) = redundant_alternatives.swap_remove(0);
            sink.push(diagnostics::pattern::redundant_arm(
                arm.pattern.get_span(),
                label,
                help,
            ));
        } else {
            for (span, label, help, _) in redundant_alternatives {
                sink.push(diagnostics::pattern::redundant_arm(span, label, help));
            }
        }

        if !arm.has_guard() {
            for current_row in current_rows {
                unguarded_previous.push((index, current_row));
            }
        }
    }

    !found_redundant
}

fn is_rejected(value: &Expression) -> bool {
    value.get_type().is_error()
}

fn has_rejected_pattern(pattern: &Pattern, rejected_spans: &HashSet<Span>) -> bool {
    if rejected_spans.contains(&pattern.get_span()) {
        return true;
    }
    match pattern {
        Pattern::EnumVariant {
            fields: patterns, ..
        }
        | Pattern::Tuple {
            elements: patterns, ..
        }
        | Pattern::Or { patterns, .. }
        | Pattern::Slice {
            prefix: patterns, ..
        } => patterns
            .iter()
            .any(|pattern| has_rejected_pattern(pattern, rejected_spans)),
        Pattern::Struct { fields, .. } => fields
            .iter()
            .any(|field| has_rejected_pattern(&field.value, rejected_spans)),
        Pattern::AsBinding { pattern, .. } => has_rejected_pattern(pattern, rejected_spans),
        _ => false,
    }
}

fn check_if_let(
    pattern: &Pattern,
    alternative: &IfLetAlternative,
    ctx: &mut PatternAnalysisContext,
) {
    if is_pattern_irrefutable(pattern, ctx.store) {
        ctx.add_issue(pattern.get_span(), IssueKind::RedundantIfLet);

        if let (Some(alternative_expression), Some(else_span)) =
            (alternative.expression(), alternative.else_span())
            && !is_trivial_expression(alternative_expression)
        {
            ctx.add_issue(else_span, IssueKind::UnreachableIfLetElse);
        }
    } else if let (Some(alternative_expression), Some(else_span)) =
        (alternative.expression(), alternative.else_span())
        && is_trivial_expression(alternative_expression)
        && !alternative_expression.is_conditional()
    {
        ctx.add_issue(else_span, IssueKind::RedundantIfLetElse);
    }
}

fn check_refutability(pattern: &Pattern, ctx: &mut PatternAnalysisContext) -> bool {
    let mut innermost = pattern;
    while let Pattern::AsBinding { pattern, .. } = innermost {
        innermost = pattern;
    }
    if matches!(innermost, Pattern::Or { .. } | Pattern::Literal { .. }) {
        return true;
    }

    let mut unions = HashMap::default();
    let norm_ctx = ctx.normalize_context();
    let row = vec![normalize_pattern(
        pattern,
        &mut unions,
        &norm_ctx,
        &mut ctx.cache,
    )];

    if let Err(witnesses) = check_exhaustiveness(&[row], &unions) {
        let witness = witnesses.first().expect("witnesses not empty");
        let witness_string = format_witness(witness);

        ctx.sink.push(diagnostics::pattern::refutable_pattern(
            pattern.get_span(),
            refutable_pattern(pattern, &witness_string),
        ));
        return false;
    }

    true
}

fn refutable_pattern<'a>(
    pattern: &Pattern,
    witness: &'a str,
) -> diagnostics::pattern::RefutablePattern<'a> {
    let pattern = match pattern {
        Pattern::AsBinding { pattern, .. } => pattern,
        pattern => pattern,
    };

    match pattern {
        Pattern::Slice { prefix, rest, .. } if rest.is_present() => {
            diagnostics::pattern::RefutablePattern::SlicePrefix(prefix.len())
        }
        Pattern::Slice { prefix, .. } => {
            diagnostics::pattern::RefutablePattern::ExactSlice(prefix.len())
        }
        Pattern::EnumVariant {
            resolution:
                ConstructorPatternResolution::EnumVariant {
                    enum_name,
                    variant_name,
                },
            ..
        } if unqualified_name(enum_name) == "Option" && variant_name == "Some" => {
            diagnostics::pattern::RefutablePattern::Some
        }
        Pattern::EnumVariant {
            resolution:
                ConstructorPatternResolution::EnumVariant {
                    enum_name,
                    variant_name,
                },
            ..
        } if unqualified_name(enum_name) == "Result" && variant_name == "Ok" => {
            diagnostics::pattern::RefutablePattern::Ok
        }
        _ => diagnostics::pattern::RefutablePattern::Other(witness),
    }
}

pub fn is_pattern_irrefutable(pattern: &Pattern, store: &Store) -> bool {
    let mut cache = InhabitanceCache::new();
    let norm_ctx = NormalizationContext {
        store,
        scrutinee_type: None,
    };

    let mut unions = HashMap::default();

    let rows: Vec<Row> = if let Pattern::Or { patterns, .. } = pattern {
        patterns
            .iter()
            .map(|alt| vec![normalize_pattern(alt, &mut unions, &norm_ctx, &mut cache)])
            .collect()
    } else {
        vec![vec![normalize_pattern(
            pattern,
            &mut unions,
            &norm_ctx,
            &mut cache,
        )]]
    };

    check_exhaustiveness(&rows, &unions).is_ok()
}
