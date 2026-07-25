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
use std::cell::RefCell;

use diagnostics::{IssueKind, LocalSink, PatternIssue};
use semantics::context::AnalysisContext;
use semantics::store::Store;
use syntax::ast::{Expression, IfLetAlternative, Literal, Pattern, SelectArmPattern, Span};
use syntax::types::Type;

use maranget::is_useful;
use normalize::normalize_arm;

pub struct PatternAnalysisContext<'a> {
    pub store: &'a Store,
    cache: InhabitanceCache,
    issues: RefCell<Vec<PatternIssue>>,
    or_pattern_error_spans: &'a HashSet<Span>,
}

impl<'a> PatternAnalysisContext<'a> {
    pub fn new(
        analysis: &'a AnalysisContext<'a>,
        or_pattern_error_spans: &'a HashSet<Span>,
    ) -> Self {
        Self {
            store: analysis.store,
            cache: InhabitanceCache::new(),
            issues: RefCell::new(Vec::new()),
            or_pattern_error_spans,
        }
    }

    fn normalize_context(&self) -> NormalizationContext<'_> {
        NormalizationContext {
            store: self.store,
            cache: &self.cache,
            scrutinee_type: None,
        }
    }

    fn normalize_context_for_match(&self, scrutinee_type: Type) -> NormalizationContext<'_> {
        NormalizationContext {
            store: self.store,
            cache: &self.cache,
            scrutinee_type: Some(scrutinee_type),
        }
    }

    fn add_issue(&self, span: Span, kind: IssueKind) {
        self.issues.borrow_mut().push(PatternIssue { span, kind });
    }

    pub fn take_issues(self) -> Vec<PatternIssue> {
        self.issues.into_inner()
    }
}

pub fn check(expression: &Expression, ctx: &PatternAnalysisContext, sink: &LocalSink) {
    match expression {
        Expression::Literal { literal, .. } => {
            if let Literal::Slice(expressions) = literal {
                for e in expressions {
                    check(e, ctx, sink);
                }
            }
        }

        Expression::Function { params, body, .. } => {
            for param in params {
                if !check_refutability(&param.pattern, ctx, sink) {
                    return;
                }
            }
            if let Some(body) = body.definition() {
                check(body, ctx, sink);
            }
        }
        Expression::Lambda { params, body, .. } => {
            for param in params {
                if !check_refutability(&param.pattern, ctx, sink) {
                    return;
                }
            }
            check(body, ctx, sink);
        }

        Expression::Block { items, .. } => {
            for e in items {
                check(e, ctx, sink);
            }
        }

        Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
            for e in items {
                check(e, ctx, sink);
            }
        }

        Expression::Let {
            binding,
            value,
            mode,
            ..
        } => {
            check(value, ctx, sink);

            if let Some(else_expression) = mode.else_block() {
                check(else_expression, ctx, sink);

                if is_pattern_irrefutable(&binding.pattern, ctx.store) {
                    ctx.add_issue(binding.pattern.get_span(), IssueKind::RedundantLetElse);
                }
            } else if mode.is_assert() {
                if is_pattern_irrefutable(&binding.pattern, ctx.store) {
                    ctx.add_issue(binding.pattern.get_span(), IssueKind::RedundantLetAssert);
                }
            } else if !check_refutability(&binding.pattern, ctx, sink) {
            }
        }

        Expression::Identifier { .. } => {}

        Expression::Call {
            expression,
            args,
            spread,
            ..
        } => {
            check(expression, ctx, sink);
            for e in args {
                check(e, ctx, sink);
            }
            if let Some(spread_expr) = spread.as_ref() {
                check(spread_expr, ctx, sink);
            }
        }

        Expression::If {
            condition,
            consequence,
            alternative,
            ..
        } => {
            check(condition, ctx, sink);
            check(consequence, ctx, sink);
            if let Some(alternative) = alternative {
                check(alternative, ctx, sink);
            }
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            ..
        } => {
            check(scrutinee, ctx, sink);
            check_if_let(pattern, alternative, ctx);
            check(consequence, ctx, sink);
            if let Some(alternative) = alternative.expression() {
                check(alternative, ctx, sink);
            }
        }

        Expression::Match {
            subject,
            arms,
            span,
            ..
        } => {
            check(subject, ctx, sink);

            if !is_inhabited(&subject.get_type(), ctx.store, &ctx.cache) {
                return;
            }

            let mut unions = HashMap::default();
            let norm_ctx = ctx.normalize_context_for_match(subject.get_type());

            let unguarded_rows: Vec<Row> = arms
                .iter()
                .filter(|arm| !arm.has_guard())
                .flat_map(|arm| normalize_arm(arm, &mut unions, &norm_ctx))
                .collect();

            if let Err(witnesses) = check_exhaustiveness(&unguarded_rows, &unions) {
                let mut cases: Vec<String> =
                    witnesses.iter().map(witness::format_witness).collect();
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

            if !check_redundancy_with_guards(arms, &mut unions, &norm_ctx, sink) {
                return;
            }

            for a in arms {
                check(&a.expression, ctx, sink);
                if let Some(guard) = &a.guard {
                    check(guard, ctx, sink);
                }
            }
        }

        Expression::Tuple { elements, .. } => {
            for e in elements {
                check(e, ctx, sink);
            }
        }

        Expression::Enum { .. } => {}
        Expression::Struct { .. } => {}
        Expression::StructCall { spread, .. } => {
            if let Some(expression) = spread.as_expression() {
                check(expression, ctx, sink);
            }
        }
        Expression::DotAccess { expression, .. } => check(expression, ctx, sink),
        Expression::Assignment { .. } => {}

        Expression::Return { expression, .. } => check(expression, ctx, sink),
        Expression::Propagate { expression, .. } => check(expression, ctx, sink),

        Expression::Interface { .. } => {}
        Expression::ImplBlock { methods, .. } => {
            for e in methods {
                check(e, ctx, sink);
            }
        }

        Expression::Binary { left, right, .. } => {
            check(left, ctx, sink);
            check(right, ctx, sink);
        }

        Expression::Paren { expression, .. } => check(expression, ctx, sink),
        Expression::Unary { expression, .. } => check(expression, ctx, sink),
        Expression::Const { expression, .. } => {
            if let Some(value) = expression.value() {
                check(value, ctx, sink);
            }
        }
        Expression::Reference { expression, .. } => check(expression, ctx, sink),
        Expression::IndexedAccess {
            expression, index, ..
        } => {
            check(expression, ctx, sink);
            check(index, ctx, sink);
        }

        Expression::Loop { body, .. } => check(body, ctx, sink),

        Expression::While {
            condition, body, ..
        } => {
            check(condition, ctx, sink);
            check(body, ctx, sink);
        }

        Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } => {
            check(scrutinee, ctx, sink);
            check(body, ctx, sink);

            if is_pattern_irrefutable(pattern, ctx.store) {
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
            if !check_refutability(&binding.pattern, ctx, sink) {
                return;
            }
            check(iterable, ctx, sink);
            check(body, ctx, sink);
        }

        Expression::Task { expression, .. } => check(expression, ctx, sink),

        Expression::Defer { expression, .. } => check(expression, ctx, sink),

        Expression::Assert { expression, .. } => check(expression, ctx, sink),

        Expression::Select { arms, .. } => {
            for arm in arms {
                match &arm.pattern {
                    SelectArmPattern::Receive {
                        receive_expression,
                        body,
                        ..
                    } => {
                        check(receive_expression.as_ref(), ctx, sink);
                        check(body.as_ref(), ctx, sink);
                    }
                    SelectArmPattern::Send {
                        send_expression,
                        body,
                    } => {
                        check(send_expression.as_ref(), ctx, sink);
                        check(body.as_ref(), ctx, sink);
                    }
                    SelectArmPattern::MatchReceive {
                        receive_expression,
                        arms: match_arms,
                    } => {
                        check(receive_expression.as_ref(), ctx, sink);
                        for match_arm in match_arms {
                            check(&match_arm.expression, ctx, sink);
                        }
                    }
                    SelectArmPattern::WildCard { body } => {
                        check(body.as_ref(), ctx, sink);
                    }
                }
            }
        }
        Expression::Range { start, end, .. } => {
            if let Some(start_expression) = start {
                check(start_expression, ctx, sink);
            }
            if let Some(end_expression) = end {
                check(end_expression, ctx, sink);
            }
        }

        Expression::Cast { expression, .. } => {
            check(expression, ctx, sink);
        }

        Expression::TypeAlias { .. } => {}
        Expression::VariableDeclaration { .. } => {}
        Expression::ModuleImport { .. } => {}
        Expression::Unit { .. } => {}
        Expression::RawGo { .. } => {}
        Expression::Break { value, .. } => {
            if let Some(v) = value {
                check(v, ctx, sink);
            }
        }
        Expression::Continue { .. } => {}
    }
}

/// Returns true if no redundancy found, false if an error was pushed to sink.
fn check_redundancy_with_guards(
    arms: &[syntax::ast::MatchArm],
    unions: &mut UnionTable,
    norm_ctx: &NormalizationContext,
    sink: &LocalSink,
) -> bool {
    let mut unguarded_previous: Vec<(usize, Row)> = vec![];
    let mut found_redundant = false;

    for (index, arm) in arms.iter().enumerate() {
        let current_rows = normalize_arm(arm, unions, norm_ctx);

        let mut current_arm_rows: Vec<Row> = vec![];

        for (alt_index, current_row) in current_rows.iter().enumerate() {
            let mut previous_rows: Vec<Row> =
                unguarded_previous.iter().map(|(_, r)| r.clone()).collect();
            previous_rows.extend(current_arm_rows.iter().cloned());

            if !is_useful(&previous_rows, current_row, unions) {
                let span = if let Pattern::Or { patterns, .. } = &arm.pattern {
                    patterns
                        .get(alt_index)
                        .map(|p| p.get_span())
                        .unwrap_or_else(|| arm.pattern.get_span())
                } else {
                    arm.pattern.get_span()
                };

                let covered_by_same_arm = current_arm_rows
                    .iter()
                    .any(|prev| !is_useful(std::slice::from_ref(prev), current_row, unions));

                let help = if covered_by_same_arm {
                    "This alternative is unreachable because it is already covered by an earlier alternative in the same arm"
                        .to_string()
                } else {
                    let covering = unguarded_previous.iter().find_map(|(orig_idx, prev)| {
                        if !is_useful(std::slice::from_ref(prev), current_row, unions) {
                            Some((*orig_idx, prev))
                        } else {
                            None
                        }
                    });

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

                sink.push(diagnostics::pattern::redundant_arm(span, label, help));
                found_redundant = true;
            }

            current_arm_rows.push(current_row.clone());
        }

        // Only unguarded arms count towards making later arms redundant.
        // Guarded arms are treated as potentially non-matching.
        if !arm.has_guard() {
            for current_row in current_rows {
                unguarded_previous.push((index, current_row));
            }
        }
    }

    !found_redundant
}

fn check_if_let(pattern: &Pattern, alternative: &IfLetAlternative, ctx: &PatternAnalysisContext) {
    // Suppress lints for patterns that already have or-pattern binding errors.
    if ctx.or_pattern_error_spans.contains(&pattern.get_span()) {
        return;
    }

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

/// Returns true if pattern is irrefutable, false if an error was pushed to sink.
fn check_refutability(pattern: &Pattern, ctx: &PatternAnalysisContext, sink: &LocalSink) -> bool {
    if matches!(pattern, Pattern::Or { .. }) {
        return true;
    }

    let mut unions = HashMap::default();
    let norm_ctx = ctx.normalize_context();
    let row = vec![normalize_pattern(pattern, &mut unions, &norm_ctx)];

    if let Err(witnesses) = check_exhaustiveness(&[row], &unions) {
        let witness = witnesses.first().expect("witnesses not empty");
        let witness_string = format_witness(witness);

        let slice_info = if let Pattern::Slice { prefix, rest, .. } = pattern {
            Some((prefix.len(), rest.is_present()))
        } else {
            None
        };

        sink.push(diagnostics::pattern::refutable_pattern(
            pattern.get_span(),
            &witness_string,
            slice_info,
        ));
        return false;
    }

    true
}

pub fn is_pattern_irrefutable(pattern: &Pattern, store: &Store) -> bool {
    let cache = InhabitanceCache::new();
    let norm_ctx = NormalizationContext {
        store,
        cache: &cache,
        scrutinee_type: None,
    };

    let mut unions = HashMap::default();

    let rows: Vec<Row> = if let Pattern::Or { patterns, .. } = pattern {
        patterns
            .iter()
            .map(|alt| vec![normalize_pattern(alt, &mut unions, &norm_ctx)])
            .collect()
    } else {
        vec![vec![normalize_pattern(pattern, &mut unions, &norm_ctx)]]
    };

    check_exhaustiveness(&rows, &unions).is_ok()
}
