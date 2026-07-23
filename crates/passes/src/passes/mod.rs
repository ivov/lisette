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
    let ((checks_diagnostics, pattern_issues), lint_outputs) = rayon::join(
        || checks::run_all(analysis, facts_ref),
        || {
            run_lints.then(|| {
                let (produced_facts, (ast_walk_diagnostics, ref_graph_output)) = rayon::join(
                    || fact_producers::run_all(analysis),
                    || {
                        rayon::join(
                            || lints::ast_walk::run(analysis, facts_ref),
                            || lints::ref_graph::run(analysis, facts_ref),
                        )
                    },
                );
                (produced_facts, ast_walk_diagnostics, ref_graph_output)
            })
        },
    );

    facts.pattern_issues = pattern_issues;

    sink.extend(checks_diagnostics);
    deferred::run(analysis.store, facts, sink);
    if let Some((produced_facts, ast_walk_diagnostics, ref_graph_output)) = lint_outputs {
        lints::from_facts::run(analysis, facts, &produced_facts, unused, sink);
        let (ref_graph_diagnostics, ref_graph_unused) = ref_graph_output;
        sink.extend(ast_walk_diagnostics);
        sink.extend(ref_graph_diagnostics);
        unused.merge(ref_graph_unused);
    }
}
