use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::program::UnusedInfo;

use crate::context::AnalysisContext;
use crate::facts::Facts;

mod ast_lints;
mod duplicate_bindings;
mod enum_variant_value;
mod generics;
mod irrefutable_patterns;
mod lints;
mod native_value_usage;
mod newtype;
pub mod pattern_analysis;
mod post_inference;
mod prelude_shadowing;
mod receivers;
mod ref_lints;
pub(crate) mod temp_producing;
mod unused_expressions;
mod visibility;

pub use lints::Lint;

pub(crate) struct ValidatorContext<'a> {
    pub typed_ast: &'a [Expression],
    pub module_id: &'a str,
    pub analysis: &'a AnalysisContext<'a>,
    pub facts: &'a mut Facts,
    pub sink: &'a LocalSink,
}

pub fn run(
    analysis: &AnalysisContext,
    facts: &mut Facts,
    sink: &LocalSink,
    unused: &mut UnusedInfo,
    run_lints: bool,
) {
    let store = analysis.store;

    for module in store.modules.values() {
        visibility::run_module(&module.id, store, sink);
        for file in module.files.values() {
            let mut ctx = ValidatorContext {
                typed_ast: &file.items,
                module_id: &module.id,
                analysis,
                facts,
                sink,
            };
            run_per_file(&mut ctx);
        }
    }

    let pattern_ctx = pattern_analysis::Context::new(analysis, &facts.or_pattern_error_spans);
    for module in store.modules.values() {
        for file in module.files.values() {
            for expression in &file.items {
                pattern_analysis::check(expression, &pattern_ctx, sink);
            }
        }
    }
    facts.pattern_issues = pattern_ctx.take_issues();

    if run_lints {
        lints::lint_all_facts(facts, unused, sink);
        lints::lint_all_modules(analysis, facts, sink, unused);
    }
}

fn run_per_file(ctx: &mut ValidatorContext<'_>) {
    let store = ctx.analysis.store;
    duplicate_bindings::run(ctx.typed_ast, ctx.sink);
    irrefutable_patterns::run(ctx.typed_ast, ctx.sink);
    receivers::run(ctx.typed_ast, ctx.sink);
    prelude_shadowing::run(ctx.typed_ast, store, ctx.sink);
    generics::run(ctx.typed_ast, ctx.module_id, store, ctx.facts, ctx.sink);
    newtype::run(ctx.typed_ast, store, ctx.sink);
    native_value_usage::run(ctx.typed_ast, ctx.module_id, store, ctx.sink);
    enum_variant_value::run(ctx.typed_ast, store, ctx.sink);
    temp_producing::run(ctx.typed_ast, ctx.sink);
    unused_expressions::run(ctx.typed_ast, ctx.module_id, store, ctx.facts);
    post_inference::run(ctx.facts, ctx.sink);
}
