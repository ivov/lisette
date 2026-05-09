pub(crate) mod generics;
pub(crate) mod unused_expressions;

use crate::context::AnalysisContext;
use crate::facts::Facts;

pub(crate) fn run_all(analysis: &AnalysisContext, facts: &mut Facts) {
    let store = analysis.store;
    for module in store.modules.values() {
        for file in module.files.values() {
            generics::run(&file.items, facts);
            unused_expressions::run(&file.items, &module.id, store, facts);
        }
    }
}
