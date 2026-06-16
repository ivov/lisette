use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::program::UnusedInfo;

use semantics::context::AnalysisContext;
use semantics::facts::Facts;

pub(crate) mod checks;
pub(crate) mod comparison;
mod deferred;
mod fact_producers;
mod lints;
pub(crate) mod walk;

pub use lints::Lint;

pub(crate) const PARALLEL_THRESHOLD: usize = 4;

pub(crate) fn is_trivial_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Unit { .. } => true,
        Expression::Block { items, .. } => {
            items.is_empty() || (items.len() == 1 && matches!(items[0], Expression::Unit { .. }))
        }
        Expression::Tuple { elements, .. } => elements.is_empty(),
        _ => false,
    }
}

pub fn run(
    analysis: &AnalysisContext,
    facts: &mut Facts,
    sink: &LocalSink,
    unused: &mut UnusedInfo,
    run_lints: bool,
) {
    let facts_ref: &Facts = facts;
    let (((checks_diagnostics, pattern_issues), producer_facts), lint_outputs) = rayon::join(
        || {
            rayon::join(
                || checks::run_all(analysis, facts_ref),
                || fact_producers::run_all(analysis),
            )
        },
        || {
            run_lints.then(|| {
                rayon::join(
                    || lints::ast_walk::run(analysis, facts_ref),
                    || lints::ref_graph::run(analysis, facts_ref),
                )
            })
        },
    );

    facts.pattern_issues = pattern_issues;
    facts.absorb_local_facts(producer_facts);

    sink.extend(checks_diagnostics);
    deferred::run(facts, sink);
    if run_lints {
        lints::from_facts::run(facts, unused, sink);
    }
    if let Some((ast_walk_diagnostics, (ref_graph_diagnostics, ref_graph_unused))) = lint_outputs {
        sink.extend(ast_walk_diagnostics);
        sink.extend(ref_graph_diagnostics);
        unused.merge(ref_graph_unused);
    }
}
