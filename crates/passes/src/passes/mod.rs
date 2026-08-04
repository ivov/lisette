use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::program::UnusedInfo;
use syntax::program::{File, Package};

use semantics::facts::Facts;
use semantics::store::Store;

pub(crate) mod checks;
pub(crate) mod comparison;
mod deferred;
mod diagnostic_producers;
mod lints;
pub(crate) mod walk;

pub use lints::Lint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintMode {
    Skip,
    Run,
}

pub(crate) const PARALLEL_THRESHOLD: usize = 4;

pub(crate) fn source_file_work(store: &semantics::store::Store) -> Vec<(&Package, &File)> {
    let mut work: Vec<_> = store
        .packages
        .values()
        .map(std::sync::Arc::as_ref)
        .flat_map(|package| package.source_files().map(move |file| (package, file)))
        .collect();
    work.sort_unstable_by(|a, b| {
        a.0.id
            .cmp(&b.0.id)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    work
}

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

pub fn run(store: &Store, facts: &Facts, sink: &LocalSink, lint_mode: LintMode) -> UnusedInfo {
    let ((checks_diagnostics, pattern_lints), lint_outputs) = rayon::join(
        || checks::run_all(store, facts, lint_mode),
        || match lint_mode {
            LintMode::Skip => None,
            LintMode::Run => {
                let (produced_facts, (ast_walk_diagnostics, ref_graph_output)) = rayon::join(
                    || diagnostic_producers::run_all(store),
                    || {
                        rayon::join(
                            || lints::ast_walk::run(store, facts),
                            || lints::ref_graph::run(store, facts),
                        )
                    },
                );
                Some((produced_facts, ast_walk_diagnostics, ref_graph_output))
            }
        },
    );

    sink.extend(checks_diagnostics);
    deferred::run(store, facts.deferred_checks(), sink);
    if let Some((produced_facts, ast_walk_diagnostics, ref_graph_output)) = lint_outputs {
        let mut unused = lints::from_facts::run(store, facts, pattern_lints, produced_facts, sink);
        let (ref_graph_diagnostics, ref_graph_unused) = ref_graph_output;
        sink.extend(ast_walk_diagnostics);
        sink.extend(ref_graph_diagnostics);
        unused.merge(ref_graph_unused);
        unused
    } else {
        UnusedInfo::default()
    }
}
