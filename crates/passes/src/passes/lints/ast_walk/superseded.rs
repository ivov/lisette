use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::Span;

use semantics::facts::Usage;
use semantics::store::Store;

pub(super) fn build_index(store: &Store) -> HashMap<Span, String> {
    let mut index = HashMap::default();
    for package in store.packages.values() {
        for definition in package.definitions.values() {
            if let (Some(name_span), Some(successor)) =
                (definition.name_span, definition.superseded_by())
            {
                index.insert(name_span, successor.to_string());
            }

            let Some(methods) = definition.methods() else {
                continue;
            };
            for method in methods.values() {
                let (Some(name_span), Some(successor)) =
                    (method.name_span, method.superseded_by.as_deref())
                else {
                    continue;
                };
                index.insert(name_span, successor.to_string());
            }
        }
    }
    index
}

pub(super) fn sweep(usages: &[&Usage], index: &HashMap<Span, String>, sink: &LocalSink) {
    for usage in usages {
        if let Some(successor) = index.get(&usage.definition_span) {
            sink.push(diagnostics::lint::superseded_api(
                &usage.usage_span,
                successor,
            ));
        }
    }
}
