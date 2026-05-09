use diagnostics::LocalSink;
use syntax::program::UnusedInfo;

use crate::context::AnalysisContext;
use crate::facts::Facts;

pub(crate) mod checks;
mod deferred;
mod fact_producers;
mod lints;

pub use lints::Lint;

pub fn run(
    analysis: &AnalysisContext,
    facts: &mut Facts,
    sink: &LocalSink,
    unused: &mut UnusedInfo,
    run_lints: bool,
) {
    checks::run_all(analysis, facts, sink);
    fact_producers::run_all(analysis, facts);
    deferred::run(facts, sink);

    if run_lints {
        lints::from_facts::run(facts, unused, sink);
        lints::ast_walk::run(analysis, sink);
        lints::replaceable_with_zero_fill::run(analysis, sink);
        lints::ref_graph::run(analysis, facts, unused, sink);
    }
}
