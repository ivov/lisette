pub(crate) mod duplicate_bindings;
pub(crate) mod enum_variant_value;
pub(crate) mod generics;
pub(crate) mod irrefutable_patterns;
pub(crate) mod native_value_usage;
pub(crate) mod newtype;
mod pattern_analysis;
pub(crate) mod prelude_shadowing;
pub(crate) mod receivers;
pub(crate) mod stringer_signature;
pub(crate) mod temp_producing;
pub(crate) mod visibility;

use diagnostics::{LocalSink, PatternIssue};
use rayon::prelude::*;
use syntax::program::{File, Module};

use crate::context::AnalysisContext;
use crate::facts::Facts;
use crate::store::Store;

use super::PARALLEL_THRESHOLD;

pub(crate) fn run_all(analysis: &AnalysisContext, facts: &mut Facts, sink: &LocalSink) {
    let store = analysis.store;

    let mut module_ids: Vec<&str> = store.modules.keys().map(String::as_str).collect();
    module_ids.sort_unstable();
    for module_id in &module_ids {
        visibility::run_module(module_id, store, sink);
    }

    let mut work: Vec<(&Module, &File)> = store
        .modules
        .values()
        .flat_map(|m| m.files.values().map(move |f| (m, f)))
        .collect();
    work.sort_unstable_by(|a, b| {
        a.0.id
            .cmp(&b.0.id)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    let or_spans = &facts.or_pattern_error_spans;

    if work.len() < PARALLEL_THRESHOLD {
        let pattern_ctx = pattern_analysis::Context::new(analysis, or_spans);
        for (module, file) in &work {
            run_file_checks(module, file, store, sink, &pattern_ctx);
        }
        facts.pattern_issues = pattern_ctx.take_issues();
        return;
    }

    type WorkerOutput = (LocalSink, Vec<PatternIssue>);
    let outputs: Vec<WorkerOutput> = work
        .par_iter()
        .map(|(module, file)| {
            let local_sink = LocalSink::new();
            let pattern_ctx = pattern_analysis::Context::new(analysis, or_spans);
            run_file_checks(module, file, store, &local_sink, &pattern_ctx);
            (local_sink, pattern_ctx.take_issues())
        })
        .collect();

    let mut worker_sinks = Vec::with_capacity(outputs.len());
    let mut all_issues = Vec::new();
    for (worker_sink, issues) in outputs {
        worker_sinks.push(worker_sink);
        all_issues.extend(issues);
    }
    sink.extend(LocalSink::merge(worker_sinks));
    facts.pattern_issues = all_issues;
}

fn run_file_checks(
    module: &Module,
    file: &File,
    store: &Store,
    sink: &LocalSink,
    pattern_ctx: &pattern_analysis::Context,
) {
    duplicate_bindings::run(&file.items, sink);
    irrefutable_patterns::run(&file.items, sink);
    receivers::run(&file.items, sink);
    stringer_signature::run(&file.items, sink);
    prelude_shadowing::run(&file.items, store, sink);
    generics::run(&file.items, &module.id, store, sink);
    newtype::run(&file.items, store, sink);
    native_value_usage::run(&file.items, &module.id, store, sink);
    enum_variant_value::run(&file.items, store, sink);
    temp_producing::run(&file.items, sink);
    for expression in &file.items {
        pattern_analysis::check(expression, pattern_ctx, sink);
    }
}
