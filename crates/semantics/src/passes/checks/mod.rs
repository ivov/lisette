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

use diagnostics::LocalSink;

use crate::context::AnalysisContext;
use crate::facts::Facts;

pub(crate) fn run_all(analysis: &AnalysisContext, facts: &mut Facts, sink: &LocalSink) {
    let store = analysis.store;
    let pattern_ctx = pattern_analysis::Context::new(analysis, &facts.or_pattern_error_spans);

    for module in store.modules.values() {
        visibility::run_module(&module.id, store, sink);
        for file in module.files.values() {
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
                pattern_analysis::check(expression, &pattern_ctx, sink);
            }
        }
    }

    facts.pattern_issues = pattern_ctx.take_issues();
}
