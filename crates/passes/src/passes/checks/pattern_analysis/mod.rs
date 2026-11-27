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

use rustc_hash::{FxHashMap, FxHashSet};

use diagnostics::{IssueKind, LocalSink};
use semantics::store::Store;
use std::slice;
use syntax::ast::MatchArm;
use syntax::ast::{
    ConstructorPatternResolution, Expression, IfLetAlternative, Literal, OrChoice, Pattern,
    SelectArm, Span, expand_or_patterns, outermost_or_patterns,
};
use syntax::types::{Type, unqualified_name};

use maranget::is_useful;
use normalize::normalize_arm;

pub struct PatternAnalysisContext<'a, 'sink> {
    pub store: &'a Store,
    cache: InhabitanceCache,
    pattern_error_spans: &'a FxHashSet<Span>,
    sink: &'sink LocalSink,
    lint_sink: Option<&'sink LocalSink>,
}

impl<'a, 'sink> PatternAnalysisContext<'a, 'sink> {
    pub fn new(
        store: &'a Store,
        pattern_error_spans: &'a FxHashSet<Span>,
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

    fn normalize_context(&self, scrutinee_type: Type) -> NormalizationContext<'a> {
        NormalizationContext::new(self.store, scrutinee_type)
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
                if !check_refutability(&param.pattern, &param.ty, ctx) {
                    return;
                }
            }
            if let Some(body) = body.definition() {
                check(body, ctx);
            }
        }
        Expression::Lambda { params, body, .. } => {
            for param in params {
                if !check_refutability(&param.pattern, &param.ty, ctx) {
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

                if is_pattern_irrefutable(&binding.pattern, &binding.ty, ctx.store) {
                    ctx.add_issue(binding.pattern.get_span(), IssueKind::RedundantLetElse);
                }
            } else if mode.is_assert() {
                if is_pattern_irrefutable(&binding.pattern, &binding.ty, ctx.store) {
                    ctx.add_issue(binding.pattern.get_span(), IssueKind::RedundantLetAssert);
                }
            } else {
                check_refutability(&binding.pattern, &binding.ty, ctx);
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
                check_if_let(pattern, &scrutinee.get_type(), alternative, ctx);
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

            let mut unions = FxHashMap::default();
            let norm_ctx = ctx.normalize_context(subject.get_type());

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
                && is_pattern_irrefutable(pattern, &scrutinee.get_type(), ctx.store)
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
            if !check_refutability(&binding.pattern, &binding.ty, ctx) {
                return;
            }
            check(iterable, ctx);
            check(body, ctx);
        }

        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive {
                        binding,
                        receive_expression,
                        body,
                    } => {
                        check(receive_expression.as_ref(), ctx);
                        check_select_payload(binding, receive_expression, ctx);
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
                            check_select_payload(&match_arm.pattern, receive_expression, ctx);
                            check(&match_arm.expression, ctx);
                        }
                    }
                    SelectArm::WildCard { body, .. } => {
                        check(body.as_ref(), ctx);
                    }
                }
            }
        }
        _ => {
            expression.for_each_child(|child| {
                check(child, ctx);
            });
        }
    }
}

fn check_redundancy_with_guards(
    arms: &[MatchArm],
    rejected_spans: &FxHashSet<Span>,
    unions: &mut UnionTable,
    norm_ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
    sink: &LocalSink,
) -> bool {
    let mut matrix: Vec<Row> = vec![];
    let mut matrix_arm_indices: Vec<usize> = vec![];
    let mut found_redundant = false;

    for (index, arm) in arms.iter().enumerate() {
        if rejected_spans.contains(&arm.pattern.get_span()) {
            continue;
        }

        let earlier_arms_len = matrix.len();
        let mut rows: Vec<TestedRow> = vec![];

        for expansion in expand_or_patterns(&arm.pattern, &|_| true) {
            let rejected = has_rejected_pattern(&expansion.pattern, rejected_spans)
                || expansion
                    .choices
                    .iter()
                    .any(|choice| rejected_spans.contains(&choice.or_span));
            if rejected {
                rows.push(TestedRow {
                    choices: expansion.choices,
                    row: None,
                    useful: true,
                });
                continue;
            }

            let row = vec![normalize_pattern(
                &expansion.pattern,
                unions,
                norm_ctx,
                cache,
            )];
            let useful = is_useful(&matrix, &row, unions);
            if !arm.has_guard() {
                matrix.push(row.clone());
                matrix_arm_indices.push(index);
            }
            rows.push(TestedRow {
                choices: expansion.choices,
                row: Some(row),
                useful,
            });
        }

        let earlier_rows = &matrix[..earlier_arms_len];
        let earlier_indices = &matrix_arm_indices[..earlier_arms_len];

        if rows.iter().all(|row| !row.useful) {
            let first_row = rows[0].row.as_ref().expect("a useless row is tested");
            let help =
                covered_by_earlier_arm_help(earlier_rows, earlier_indices, first_row, unions);
            sink.push(diagnostics::pattern::redundant_arm(
                arm.pattern.get_span(),
                format!("arm #{} is unreachable", index + 1),
                help,
            ));
            found_redundant = true;
        } else {
            for alternative_span in redundant_alternatives(&rows) {
                let first_row = rows
                    .iter()
                    .find(|row| {
                        row.choices
                            .iter()
                            .any(|choice| choice.alternative_span == alternative_span)
                    })
                    .and_then(|row| row.row.as_ref())
                    .expect("a useless row is tested");
                let help = if is_useful(earlier_rows, first_row, unions) {
                    "This alternative is unreachable because it is already covered by an earlier alternative in the same arm"
                        .to_string()
                } else {
                    covered_by_earlier_arm_help(earlier_rows, earlier_indices, first_row, unions)
                };
                sink.push(diagnostics::pattern::redundant_arm(
                    alternative_span,
                    "this alternative is unreachable".to_string(),
                    help,
                ));
                found_redundant = true;
            }
        }
    }

    !found_redundant
}

struct TestedRow {
    choices: Vec<OrChoice>,
    /// `None` for a row that the checker rejected, which counts as useful.
    row: Option<Row>,
    useful: bool,
}

/// An alternative inside a redundant alternative is not listed.
fn redundant_alternatives(rows: &[TestedRow]) -> Vec<Span> {
    let mut alternatives: Vec<(Span, bool)> = vec![];
    for row in rows {
        for choice in &row.choices {
            match alternatives
                .iter_mut()
                .find(|(span, _)| *span == choice.alternative_span)
            {
                Some((_, useful)) => *useful |= row.useful,
                None => alternatives.push((choice.alternative_span, row.useful)),
            }
        }
    }

    let redundant: Vec<Span> = alternatives
        .into_iter()
        .filter(|(_, useful)| !useful)
        .map(|(span, _)| span)
        .collect();

    redundant
        .iter()
        .copied()
        .filter(|inner| {
            !redundant
                .iter()
                .any(|outer| outer != inner && outer.contains(inner))
        })
        .collect()
}

fn covered_by_earlier_arm_help(
    earlier_rows: &[Row],
    earlier_indices: &[usize],
    row: &Row,
    unions: &UnionTable,
) -> String {
    let covering = earlier_rows
        .iter()
        .zip(earlier_indices)
        .find(|(earlier, _)| !is_useful(slice::from_ref(*earlier), row, unions));

    if let Some((covering_row, covering_index)) = covering {
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
        "This pattern is covered by earlier match arms and will never be reached".to_string()
    }
}

fn is_rejected(value: &Expression) -> bool {
    value.get_type().is_error()
}

fn has_rejected_pattern(pattern: &Pattern, rejected_spans: &FxHashSet<Span>) -> bool {
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
    scrutinee_type: &Type,
    alternative: &IfLetAlternative,
    ctx: &mut PatternAnalysisContext,
) {
    if is_pattern_irrefutable(pattern, scrutinee_type, ctx.store) {
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

fn check_refutability(
    pattern: &Pattern,
    scrutinee_type: &Type,
    ctx: &mut PatternAnalysisContext,
) -> bool {
    let mut innermost = pattern;
    while let Pattern::AsBinding { pattern, .. } = innermost {
        innermost = pattern;
    }
    if matches!(innermost, Pattern::Literal { .. }) || !outermost_or_patterns(pattern).is_empty() {
        return true;
    }

    let mut unions = FxHashMap::default();
    let norm_ctx = ctx.normalize_context(scrutinee_type.clone());
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

fn check_select_payload(
    pattern: &Pattern,
    receive_expression: &Expression,
    ctx: &PatternAnalysisContext,
) {
    let Pattern::EnumVariant {
        identifier, fields, ..
    } = pattern
    else {
        return;
    };
    let [payload] = fields.as_slice() else {
        return;
    };
    let received = receive_expression.get_type();
    if unqualified_name(identifier) != "Some"
        || !received.is_option()
        || has_rejected_pattern(pattern, ctx.pattern_error_spans)
    {
        return;
    }
    if !is_pattern_irrefutable(payload, &received.ok_type(), ctx.store) {
        ctx.sink
            .push(diagnostics::infer::select_receive_refutable_pattern(
                payload.get_span(),
            ));
    }
}

pub fn is_pattern_irrefutable(pattern: &Pattern, scrutinee_type: &Type, store: &Store) -> bool {
    let mut cache = InhabitanceCache::new();
    let norm_ctx = NormalizationContext::new(store, scrutinee_type.clone());

    let mut unions = FxHashMap::default();

    let rows: Vec<Row> = expand_or_patterns(pattern, &|_| true)
        .iter()
        .map(|expansion| {
            vec![normalize_pattern(
                &expansion.pattern,
                &mut unions,
                &norm_ctx,
                &mut cache,
            )]
        })
        .collect();

    check_exhaustiveness(&rows, &unions).is_ok()
}
