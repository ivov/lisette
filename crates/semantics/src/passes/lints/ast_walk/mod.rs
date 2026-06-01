pub(crate) mod attributes;
pub(crate) mod casing;
mod checks;
pub(crate) mod visitor;

use std::cell::RefCell;

use crate::context::AnalysisContext;
use crate::facts::Facts;
use crate::passes::PARALLEL_THRESHOLD;
use crate::store::Store;
use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use rayon::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::{Expression, Pattern};
use syntax::program::{File, Module};

pub struct LintContext<'a> {
    pub ast: &'a [Expression],
    pub source: &'a str,
    pub is_d_lis: bool,
    pub files: &'a HashMap<u32, File>,
    pub module_id: &'a str,
    pub store: &'a Store,
    pub facts: &'a Facts,
}

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
        let ctx = LintContext {
            ast: &file.items,
            source: &file.source,
            is_d_lis: file.is_d_lis(),
            files: &module.files,
            module_id: &module.id,
            store,
            facts,
        };
        let mut diagnostics = AstLintGroup.check(&ctx);
        diagnostics.sort_by(LisetteDiagnostic::sort_key);
        sink.extend(diagnostics);
    }
}

use attributes::{check_attributes, check_enum_attributes, check_struct_attributes};
use checks::{
    check_bool_literal_comparison, check_double_negation, check_dup_arg, check_duplicate_cutset,
    check_duplicate_logical_operand, check_empty_match_arm, check_excess_parens_on_condition,
    check_expression_naming, check_identical_if_branches, check_identical_match_arms,
    check_invisible_in_string_expression, check_invisible_in_string_pattern, check_loop_runs_once,
    check_lost_query_mutation, check_manual_map, check_manual_unwrap_or,
    check_match_literal_collection, check_needless_bool, check_needless_range_loop,
    check_needless_return, check_pattern_naming, check_redundant_closure,
    check_redundant_pattern_matching, check_replaceable_with_zero_fill,
    check_rest_only_slice_pattern, check_self_assignment, check_self_comparison,
    check_single_arm_match, check_uninterpolated_fstring, check_unnecessary_raw_string_expression,
    check_unnecessary_raw_string_pattern, check_unsigned_comparison,
    check_verbose_failure_propagation, check_waitgroup_add_in_task,
};
use visitor::visit_ast;

type ExpressionCheck = fn(&Expression, &mut Vec<LisetteDiagnostic>);
type PatternCheck = fn(&Pattern, &mut Vec<LisetteDiagnostic>);

const EXPRESSION_CHECKS: &[ExpressionCheck] = &[
    check_double_negation,
    check_self_comparison,
    check_unsigned_comparison,
    check_self_assignment,
    check_bool_literal_comparison,
    check_identical_if_branches,
    check_identical_match_arms,
    check_loop_runs_once,
    check_empty_match_arm,
    check_excess_parens_on_condition,
    check_match_literal_collection,
    check_needless_bool,
    check_needless_range_loop,
    check_needless_return,
    check_single_arm_match,
    check_redundant_pattern_matching,
    check_manual_map,
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
];

const PATTERN_CHECKS: &[PatternCheck] = &[
    check_rest_only_slice_pattern,
    check_unnecessary_raw_string_pattern,
    check_invisible_in_string_pattern,
];

pub struct AstLintGroup;

impl AstLintGroup {
    pub fn check(&self, ctx: &LintContext) -> Vec<LisetteDiagnostic> {
        let diagnostics = RefCell::new(Vec::new());
        let is_d_lis = ctx.is_d_lis;
        let files = ctx.files;
        let store = ctx.store;
        let module_id = ctx.module_id;
        let source = ctx.source;
        let facts = ctx.facts;

        visit_ast(
            ctx.ast,
            &mut |expression| {
                let mut sink = diagnostics.borrow_mut();
                for check in EXPRESSION_CHECKS {
                    check(expression, &mut sink);
                }
                check_duplicate_logical_operand(expression, files, &mut sink);
                check_expression_naming(expression, is_d_lis, &mut sink);
                check_replaceable_with_zero_fill(expression, store, module_id, source, &mut sink);
                check_lost_query_mutation(expression, store, &mut sink);
                check_redundant_closure(expression, facts, &mut sink);
            },
            &mut |pattern| {
                let mut sink = diagnostics.borrow_mut();
                for check in PATTERN_CHECKS {
                    check(pattern, &mut sink);
                }
                check_pattern_naming(pattern, is_d_lis, &mut sink);
            },
        );

        diagnostics.into_inner()
    }
}
