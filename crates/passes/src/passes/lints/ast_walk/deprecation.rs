use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::Span;
use syntax::types::Type;

use semantics::facts::Usage;
use semantics::store::Store;

pub(super) fn build_index(store: &Store) -> HashMap<Span, String> {
    let mut index = HashMap::default();
    for module in store.modules.values() {
        for definition in module.definitions.values() {
            if !is_function_type(&definition.ty) {
                continue;
            }
            let Some(name_span) = definition.name_span() else {
                continue;
            };
            let Some(doc) = definition.doc() else {
                continue;
            };
            if let Some(message) = deprecation_message(doc) {
                index.insert(name_span, message);
            }
        }
    }
    index
}

fn is_function_type(ty: &Type) -> bool {
    match ty {
        Type::Function(_) => true,
        Type::Forall { body, .. } => is_function_type(body),
        _ => false,
    }
}

pub(super) fn sweep(usages: &[&Usage], index: &HashMap<Span, String>, sink: &LocalSink) {
    for usage in usages {
        if let Some(message) = index.get(&usage.definition_span) {
            sink.push(diagnostics::lint::deprecated_api(
                &usage.usage_span,
                message,
            ));
        }
    }
}

fn deprecation_message(doc: &str) -> Option<String> {
    if !doc.contains("Deprecated:") {
        return None;
    }

    let mut paragraph = doc
        .lines()
        .map(str::trim)
        .skip_while(|line| !line.starts_with("Deprecated:"))
        .take_while(|line| !line.is_empty());

    let mut message = paragraph
        .next()?
        .trim_start_matches("Deprecated:")
        .trim()
        .to_string();
    for line in paragraph {
        message.push(' ');
        message.push_str(line);
    }

    if message.is_empty() {
        return None;
    }

    let mut chars = message.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => message,
    };
    Some(capitalized)
}
