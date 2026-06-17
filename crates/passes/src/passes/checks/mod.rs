pub(crate) mod cast_nan_to_int;
pub(crate) mod const_naming;
pub(crate) mod decimal_file_mode;
pub(crate) mod duplicate_bindings;
pub(crate) mod empty_infinite_loop;
pub(crate) mod empty_range;
pub(crate) mod empty_select_default;
pub(crate) mod enum_variant_value;
pub(crate) mod generics;
pub(crate) mod impossible_comparison;
pub(crate) mod index_out_of_bounds;
pub(crate) mod interpolation_stringer;
pub(crate) mod irrefutable_patterns;
pub(crate) mod json_methods;
pub(crate) mod json_serializable_fields;
pub(crate) mod min_max;
pub(crate) mod nan_comparison;
pub(crate) mod native_value_usage;
pub(crate) mod newtype;
mod node_walk;
pub(crate) mod oversized_shift;
mod pattern_analysis;
pub(crate) mod predeclared_shadowing;
pub(crate) mod prelude_shadowing;
pub(crate) mod pub_type_export;
pub(crate) mod receivers;
pub(crate) mod repeated_if_condition;
pub(crate) mod stringer_signature;
pub(crate) mod temp_producing;
pub(crate) mod unchanging_loop_condition;
pub(crate) mod visibility;

use diagnostics::{LisetteDiagnostic, LocalSink, PatternIssue};
use rayon::prelude::*;
use rustc_hash::FxHashSet as HashSet;
use std::sync::Arc;
use syntax::program::{File, Module};

use crate::passes::walk::NodeCtx;
use semantics::context::AnalysisContext;
use semantics::facts::Facts;
use semantics::store::Store;

use super::PARALLEL_THRESHOLD;

pub(crate) fn run_all(
    analysis: &AnalysisContext,
    facts: &Facts,
) -> (Vec<LisetteDiagnostic>, Vec<PatternIssue>) {
    let store = analysis.store;
    let sink = LocalSink::new();

    let mut module_ids: Vec<&str> = store.modules.keys().map(String::as_str).collect();
    module_ids.sort_unstable();
    for module_id in &module_ids {
        visibility::run_module(module_id, store, &sink);
        json_methods::run_module(module_id, store, &sink);
    }

    let mut work: Vec<(&Module, &File)> = store
        .modules
        .values()
        .map(Arc::as_ref)
        .flat_map(|m| m.files.values().map(move |f| (m, f)))
        .collect();
    work.sort_unstable_by(|a, b| {
        a.0.id
            .cmp(&b.0.id)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    let or_spans = &facts.or_pattern_error_spans;

    let ufcs_methods = analysis.ufcs_methods;

    if work.len() < PARALLEL_THRESHOLD {
        let pattern_ctx = pattern_analysis::Context::new(analysis, or_spans);
        for (module, file) in &work {
            run_file_checks(
                module,
                file,
                store,
                facts,
                ufcs_methods,
                &sink,
                &pattern_ctx,
            );
        }
        return (sink.take(), pattern_ctx.take_issues());
    }

    type WorkerOutput = (LocalSink, Vec<PatternIssue>);
    let outputs: Vec<WorkerOutput> = work
        .par_iter()
        .map(|(module, file)| {
            let local_sink = LocalSink::new();
            let pattern_ctx = pattern_analysis::Context::new(analysis, or_spans);
            run_file_checks(
                module,
                file,
                store,
                facts,
                ufcs_methods,
                &local_sink,
                &pattern_ctx,
            );
            (local_sink, pattern_ctx.take_issues())
        })
        .collect();

    let mut worker_sinks = Vec::with_capacity(outputs.len());
    let mut all_issues = Vec::new();
    for (worker_sink, issues) in outputs {
        worker_sinks.push(worker_sink);
        all_issues.extend(issues);
    }
    let mut diagnostics = sink.take();
    diagnostics.extend(LocalSink::merge(worker_sinks));
    (diagnostics, all_issues)
}

fn run_file_checks(
    module: &Module,
    file: &File,
    store: &Store,
    facts: &Facts,
    ufcs_methods: &HashSet<(String, String)>,
    sink: &LocalSink,
    pattern_ctx: &pattern_analysis::Context,
) {
    let ctx = NodeCtx {
        store,
        facts,
        files: &module.files,
        module_id: &module.id,
        source: &file.source,
        is_d_lis: file.is_d_lis(),
        sink,
        claimed_spans: Default::default(),
    };
    node_walk::run(&file.items, &ctx);
    interpolation_stringer::run(&file.items, store, ufcs_methods, sink);

    prelude_shadowing::run(&file.items, store, sink);
    generics::run(&file.items, &module.id, store, sink);
    native_value_usage::run(&file.items, &module.id, store, sink);
    json_serializable_fields::run(&file.items, sink);
    empty_select_default::run(&file.items, sink);

    for expression in &file.items {
        pattern_analysis::check(expression, pattern_ctx, sink);
    }
}
