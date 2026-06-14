pub(crate) mod attributes;
pub(crate) mod casing;
mod checks;
mod deprecation;
mod suppression;

use std::sync::{Arc, LazyLock};

use crate::context::AnalysisContext;
use crate::facts::{Facts, Usage};
use crate::passes::PARALLEL_THRESHOLD;
use crate::passes::walk::{CheckTable, NodeCtx, walk_nodes};
use crate::store::Store;
use diagnostics::{LisetteDiagnostic, LocalSink};
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::Span;
use syntax::program::Module;

use attributes::{check_attributes, check_enum_attributes, check_struct_attributes};
use checks::{
    check_bad_bit_mask, check_bool_literal_comparison, check_collapsible_if,
    check_double_comparison, check_double_negation, check_dup_arg, check_duplicate_cutset,
    check_duplicate_logical_operand, check_empty_match_arm, check_equal_operands,
    check_excess_parens_on_condition, check_exit_after_defer, check_expression_naming,
    check_goos_goarch_comparison, check_identical_if_branches, check_identical_match_arms,
    check_ineffective_bit_mask, check_integer_division_to_zero,
    check_invisible_in_string_expression, check_invisible_in_string_pattern, check_let_and_return,
    check_loop_runs_once, check_lost_cancel, check_lost_query_mutation, check_manual_bytes_equal,
    check_manual_compound_assignment, check_manual_equal_fold, check_manual_find,
    check_manual_is_empty, check_manual_map, check_manual_map_or, check_manual_replace_all,
    check_manual_time_since, check_manual_time_until, check_manual_unwrap_or,
    check_match_as_if_let, check_match_literal_collection, check_match_on_bool,
    check_match_single_binding, check_negated_equality, check_non_negative_comparison,
    check_out_of_domain_value, check_pattern_naming, check_redundant_closure,
    check_redundant_comparison, check_redundant_else, check_redundant_operation,
    check_redundant_pattern_matching, check_redundant_slice_bounds, check_redundant_sprintf,
    check_regexp_in_loop, check_replaceable_with_zero_fill, check_rest_only_slice_pattern,
    check_self_assignment, check_self_comparison, check_single_arm_select,
    check_type_limit_comparison, check_uninterpolated_fstring, check_unnecessary_bool,
    check_unnecessary_range_loop, check_unnecessary_raw_string_expression,
    check_unnecessary_raw_string_pattern, check_unnecessary_return, check_unsigned_comparison,
    check_verbose_failure_propagation, check_waitgroup_add_in_task,
};

static LINT_CHECKS: LazyLock<CheckTable> = LazyLock::new(|| {
    use syntax::ast::ExpressionKind::*;
    use syntax::ast::PatternKind;

    CheckTable::new(
        &[
            (check_double_negation, &[Unary]),
            (check_self_comparison, &[Binary]),
            (check_redundant_comparison, &[Binary]),
            (check_double_comparison, &[Binary]),
            (check_bad_bit_mask, &[Binary]),
            (check_ineffective_bit_mask, &[Binary]),
            (check_equal_operands, &[Binary]),
            (check_unsigned_comparison, &[Binary]),
            (check_type_limit_comparison, &[Binary]),
            (check_non_negative_comparison, &[Binary]),
            (check_goos_goarch_comparison, &[Binary]),
            (check_redundant_operation, &[Binary]),
            (check_integer_division_to_zero, &[Binary]),
            (check_self_assignment, &[Assignment]),
            (check_manual_compound_assignment, &[Assignment]),
            (check_manual_bytes_equal, &[Binary]),
            (check_manual_replace_all, &[Call]),
            (check_redundant_sprintf, &[Call]),
            (check_manual_equal_fold, &[Binary]),
            (check_manual_find, &[Call]),
            (check_manual_is_empty, &[Binary]),
            (check_bool_literal_comparison, &[Binary]),
            (check_identical_if_branches, &[If]),
            (check_collapsible_if, &[If]),
            (check_identical_match_arms, &[Match]),
            (check_loop_runs_once, &[Loop, While, WhileLet, For]),
            (check_regexp_in_loop, &[Loop, While, WhileLet, For]),
            (check_empty_match_arm, &[Match]),
            (check_excess_parens_on_condition, &[If, While, Match]),
            (check_match_literal_collection, &[Match]),
            (check_match_on_bool, &[Match]),
            (check_match_single_binding, &[Match]),
            (check_negated_equality, &[Unary]),
            (check_let_and_return, &[Block]),
            (check_unnecessary_bool, &[If]),
            (check_unnecessary_range_loop, &[For]),
            (check_unnecessary_return, &[Function]),
            (check_match_as_if_let, &[Match]),
            (check_single_arm_select, &[Select]),
            (check_redundant_slice_bounds, &[IndexedAccess]),
            (check_redundant_pattern_matching, &[Match]),
            (check_manual_map, &[Match]),
            (check_manual_map_or, &[Match]),
            (check_manual_time_since, &[Call]),
            (check_manual_time_until, &[Call]),
            (check_manual_unwrap_or, &[Match]),
            (check_uninterpolated_fstring, &[Literal]),
            (check_unnecessary_raw_string_expression, &[Literal]),
            (check_invisible_in_string_expression, &[Literal]),
            (check_verbose_failure_propagation, &[Match]),
            (check_dup_arg, &[Call]),
            (check_duplicate_cutset, &[Call]),
            (check_waitgroup_add_in_task, &[Function, Lambda]),
            (check_exit_after_defer, &[Function, Lambda]),
            (check_struct_attributes, &[Struct]),
            (check_attributes, &[Function]),
            (check_enum_attributes, &[Enum]),
            (check_duplicate_logical_operand, &[Binary]),
            (
                check_expression_naming,
                &[Struct, Enum, TypeAlias, Interface, Function],
            ),
            (check_replaceable_with_zero_fill, &[StructCall]),
            (check_lost_query_mutation, &[Call]),
            (check_lost_cancel, &[Let]),
            (check_redundant_closure, &[Lambda]),
            (check_redundant_else, &[Block]),
            (check_out_of_domain_value, &[Literal, Unary, Call]),
        ],
        &[
            (
                check_rest_only_slice_pattern,
                &[PatternKind::Slice, PatternKind::Or],
            ),
            (
                check_unnecessary_raw_string_pattern,
                &[PatternKind::Literal],
            ),
            (check_invisible_in_string_pattern, &[PatternKind::Literal]),
            (check_pattern_naming, &[PatternKind::Identifier]),
        ],
    )
});

pub(crate) fn run(analysis: &AnalysisContext, facts: &Facts) -> Vec<LisetteDiagnostic> {
    let store = analysis.store;

    let deprecated = deprecation::build_index(store);
    let mut usages_by_file: HashMap<u32, Vec<&Usage>> = HashMap::default();
    if !deprecated.is_empty() {
        for usage in &facts.usages {
            usages_by_file
                .entry(usage.usage_span.file_id)
                .or_default()
                .push(usage);
        }
    }

    let mut modules: Vec<&Module> = store
        .modules
        .values()
        .map(Arc::as_ref)
        .filter(|m| !m.is_internal())
        .collect();
    modules.sort_unstable_by(|a, b| a.id.cmp(&b.id));

    if modules.len() < PARALLEL_THRESHOLD {
        let sink = LocalSink::new();
        for module in &modules {
            run_module(module, store, facts, &sink, &deprecated, &usages_by_file);
        }
        return sink.take();
    }

    let worker_sinks: Vec<LocalSink> = modules
        .par_iter()
        .map(|module| {
            let local_sink = LocalSink::new();
            run_module(
                module,
                store,
                facts,
                &local_sink,
                &deprecated,
                &usages_by_file,
            );
            local_sink
        })
        .collect();
    LocalSink::merge(worker_sinks)
}

fn run_module(
    module: &Module,
    store: &Store,
    facts: &Facts,
    sink: &LocalSink,
    deprecated: &HashMap<Span, String>,
    usages_by_file: &HashMap<u32, Vec<&Usage>>,
) {
    for (file_id, file) in &module.files {
        let file_sink = LocalSink::new();
        let ctx = NodeCtx {
            store,
            facts,
            files: &module.files,
            module_id: &module.id,
            source: &file.source,
            is_d_lis: file.is_d_lis(),
            sink: &file_sink,
            claimed_spans: Default::default(),
        };
        walk_nodes(&file.items, &ctx, &LINT_CHECKS);

        if let Some(usages) = usages_by_file.get(file_id) {
            deprecation::sweep(usages, deprecated, &file_sink);
        }

        let produced = file_sink.take();
        if produced.is_empty() {
            continue;
        }
        let allows = suppression::collect_declaration_allows(&file.items);
        sink.extend(suppression::filter_allowed(produced, &allows));
    }
}
