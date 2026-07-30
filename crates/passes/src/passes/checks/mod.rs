pub(crate) mod always_true_disjunction;
pub(crate) mod cast_nan_to_int;
pub(crate) mod const_naming;
pub(crate) mod constant_cast_overflow;
pub(crate) mod decimal_file_mode;
pub(crate) mod duplicate_bindings;
pub(crate) mod duplicate_map_keys;
pub(crate) mod empty_infinite_loop;
pub(crate) mod empty_range;
mod empty_select_default;
pub(crate) mod enum_variant_value;
pub(crate) mod generics;
pub(crate) mod impossible_comparison;
pub(crate) mod index_out_of_bounds;
pub(crate) mod instantiation_cycle;
mod interpolation_stringer;
pub(crate) mod irrefutable_patterns;
mod json_methods;
mod json_serializable_fields;
pub(crate) mod map_key;
pub(crate) mod min_max;
pub(crate) mod nan_comparison;
mod native_value_usage;
pub(crate) mod newtype;
mod node_walk;
pub(crate) mod oversized_shift;
mod pattern_analysis;
mod prelude_shadowing;
pub(crate) mod pub_type_export;
pub(crate) mod receivers;
pub(crate) mod repeated_if_condition;
pub(crate) mod stringer_signature;
pub(crate) mod temp_producing;
pub(crate) mod unchanging_loop_condition;
mod visibility;

use diagnostics::{LisetteDiagnostic, LocalSink};
use rayon::prelude::*;
use syntax::program::{File, Module};

use crate::passes::walk::NodeCtx;
use semantics::facts::Facts;
use semantics::store::Store;

use super::{PARALLEL_THRESHOLD, source_file_work};

pub(crate) fn run_all(
    store: &Store,
    facts: &Facts,
    run_lints: bool,
) -> (Vec<LisetteDiagnostic>, Vec<LisetteDiagnostic>) {
    let sink = LocalSink::new();
    let pattern_lint_sink = LocalSink::new();

    let mut module_ids: Vec<&str> = store.modules.keys().map(String::as_str).collect();
    module_ids.sort_unstable();
    for module_id in &module_ids {
        visibility::run_module(module_id, store, &sink);
        json_methods::run_module(module_id, store, &sink);
    }
    instantiation_cycle::run(store, &sink);

    let work = source_file_work(store);

    let or_spans = &facts.or_pattern_error_spans;

    if work.len() < PARALLEL_THRESHOLD {
        let mut pattern_ctx = pattern_analysis::Context::new(
            store,
            or_spans,
            &sink,
            run_lints.then_some(&pattern_lint_sink),
        );
        for (module, file) in &work {
            run_file_checks(module, file, store, facts, &mut pattern_ctx);
        }
        return (
            sink.into_diagnostics(),
            pattern_lint_sink.into_diagnostics(),
        );
    }

    let worker_sinks: Vec<(LocalSink, LocalSink)> = work
        .par_iter()
        .map(|(module, file)| {
            let local_sink = LocalSink::new();
            let local_pattern_lint_sink = LocalSink::new();
            let mut pattern_ctx = pattern_analysis::Context::new(
                store,
                or_spans,
                &local_sink,
                run_lints.then_some(&local_pattern_lint_sink),
            );
            run_file_checks(module, file, store, facts, &mut pattern_ctx);
            drop(pattern_ctx);
            (local_sink, local_pattern_lint_sink)
        })
        .collect();

    let (worker_sinks, worker_pattern_lint_sinks): (Vec<_>, Vec<_>) =
        worker_sinks.into_iter().unzip();
    let mut diagnostics = sink.into_diagnostics();
    diagnostics.extend(LocalSink::merge(worker_sinks));
    let mut pattern_lints = pattern_lint_sink.into_diagnostics();
    pattern_lints.extend(LocalSink::merge(worker_pattern_lint_sinks));
    (diagnostics, pattern_lints)
}

fn run_file_checks(
    module: &Module,
    file: &File,
    store: &Store,
    facts: &Facts,
    pattern_ctx: &mut pattern_analysis::Context,
) {
    let sink = pattern_ctx.sink();
    let mut ctx = NodeCtx {
        store,
        facts,
        files: &module.files,
        module_id: &module.id,
        source: &file.source,
        is_d_lis: file.is_d_lis(),
        is_test: file.is_test(),
        sink,
        claimed_spans: Default::default(),
    };
    node_walk::run(&file.items, &mut ctx);
    interpolation_stringer::run(&file.items, store, sink);

    prelude_shadowing::run(&file.items, store, sink);
    generics::run(&file.items, store, sink);
    native_value_usage::run(&file.items, &module.id, store, sink);
    json_serializable_fields::run(&file.items, sink);
    empty_select_default::run(&file.items, sink);

    for expression in &file.items {
        pattern_analysis::check(expression, pattern_ctx);
    }
}
