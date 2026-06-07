pub(crate) mod attributes;
pub(crate) mod casing;
mod checks;

use crate::context::AnalysisContext;
use crate::facts::Facts;
use crate::passes::PARALLEL_THRESHOLD;
use crate::passes::walk::{NodeCheck, NodeCtx, PatternCheck, walk_nodes};
use crate::store::Store;
use diagnostics::LocalSink;
use rayon::prelude::*;
use syntax::program::Module;

use attributes::{check_attributes, check_enum_attributes, check_struct_attributes};
use checks::{
    check_bool_literal_comparison, check_collapsible_if, check_double_negation, check_dup_arg,
    check_duplicate_cutset, check_duplicate_logical_operand, check_empty_match_arm,
    check_excess_parens_on_condition, check_expression_naming, check_goos_goarch_comparison,
    check_identical_if_branches, check_identical_match_arms, check_integer_division_to_zero,
    check_invisible_in_string_expression, check_invisible_in_string_pattern, check_let_and_return,
    check_loop_runs_once, check_lost_query_mutation, check_manual_bytes_equal,
    check_manual_compound_assignment, check_manual_equal_fold, check_manual_find,
    check_manual_is_empty, check_manual_map, check_manual_map_or, check_manual_replace_all,
    check_manual_time_since, check_manual_time_until, check_manual_unwrap_or,
    check_match_literal_collection, check_match_on_bool, check_match_single_binding,
    check_negated_equality, check_non_negative_comparison, check_out_of_domain_value,
    check_pattern_naming, check_redundant_closure, check_redundant_else, check_redundant_operation,
    check_redundant_pattern_matching, check_redundant_slice_bounds, check_redundant_sprintf,
    check_replaceable_with_zero_fill, check_rest_only_slice_pattern, check_self_assignment,
    check_self_comparison, check_single_arm_match, check_single_arm_select,
    check_type_limit_comparison, check_uninterpolated_fstring, check_unnecessary_bool,
    check_unnecessary_range_loop, check_unnecessary_raw_string_expression,
    check_unnecessary_raw_string_pattern, check_unnecessary_return, check_unsigned_comparison,
    check_verbose_failure_propagation, check_waitgroup_add_in_task,
};

const EXPRESSION_CHECKS: &[NodeCheck] = &[
    check_double_negation,
    check_self_comparison,
    check_unsigned_comparison,
    check_type_limit_comparison,
    check_non_negative_comparison,
    check_goos_goarch_comparison,
    check_redundant_operation,
    check_integer_division_to_zero,
    check_self_assignment,
    check_manual_compound_assignment,
    check_manual_bytes_equal,
    check_manual_replace_all,
    check_redundant_sprintf,
    check_manual_equal_fold,
    check_manual_find,
    check_manual_is_empty,
    check_bool_literal_comparison,
    check_identical_if_branches,
    check_collapsible_if,
    check_identical_match_arms,
    check_loop_runs_once,
    check_empty_match_arm,
    check_excess_parens_on_condition,
    check_match_literal_collection,
    check_match_on_bool,
    check_match_single_binding,
    check_negated_equality,
    check_let_and_return,
    check_unnecessary_bool,
    check_unnecessary_range_loop,
    check_unnecessary_return,
    check_single_arm_match,
    check_single_arm_select,
    check_redundant_slice_bounds,
    check_redundant_pattern_matching,
    check_manual_map,
    check_manual_map_or,
    check_manual_time_since,
    check_manual_time_until,
    check_manual_unwrap_or,
    check_uninterpolated_fstring,
    check_unnecessary_raw_string_expression,
    check_invisible_in_string_expression,
    check_verbose_failure_propagation,
    check_dup_arg,
    check_duplicate_cutset,
    check_waitgroup_add_in_task,
    check_struct_attributes,
    check_attributes,
    check_enum_attributes,
    check_duplicate_logical_operand,
    check_expression_naming,
    check_replaceable_with_zero_fill,
    check_lost_query_mutation,
    check_redundant_closure,
    check_redundant_else,
    check_out_of_domain_value,
];

const PATTERN_CHECKS: &[PatternCheck] = &[
    check_rest_only_slice_pattern,
    check_unnecessary_raw_string_pattern,
    check_invisible_in_string_pattern,
    check_pattern_naming,
];

pub(crate) fn run(analysis: &AnalysisContext, facts: &Facts, sink: &LocalSink) {
    let store = analysis.store;

    let mut modules: Vec<&Module> = store
        .modules
        .values()
        .filter(|m| !m.is_internal())
        .collect();
    modules.sort_unstable_by(|a, b| a.id.cmp(&b.id));

    if modules.len() < PARALLEL_THRESHOLD {
        for module in &modules {
            run_module(module, store, facts, sink);
        }
        return;
    }

    let worker_sinks: Vec<LocalSink> = modules
        .par_iter()
        .map(|module| {
            let local_sink = LocalSink::new();
            run_module(module, store, facts, &local_sink);
            local_sink
        })
        .collect();
    sink.extend(LocalSink::merge(worker_sinks));
}

fn run_module(module: &Module, store: &Store, facts: &Facts, sink: &LocalSink) {
    for file in module.files.values() {
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
        walk_nodes(&file.items, &ctx, EXPRESSION_CHECKS, PATTERN_CHECKS);
    }
}
