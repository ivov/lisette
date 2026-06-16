//! Reject impl methods named `MarshalJSON` / `UnmarshalJSON` on an enum that
//! carries `#[json]`. The attribute makes emit generate both receiver methods
//! unconditionally, so a hand-written method of the same name yields two Go
//! methods with one name and fails to compile with "method redeclared".

use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::Expression;

use semantics::store::Store;

pub(crate) fn run_module(module_id: &str, store: &Store, sink: &LocalSink) {
    let Some(module) = store.get_module(module_id) else {
        return;
    };

    let json_enums: HashSet<&str> = module
        .files
        .values()
        .flat_map(|file| file.items.iter())
        .filter_map(|item| match item {
            Expression::Enum {
                name, attributes, ..
            } if attributes.iter().any(|a| a.name == "json") => Some(name.as_str()),
            _ => None,
        })
        .collect();

    if json_enums.is_empty() {
        return;
    }

    for item in module.files.values().flat_map(|file| file.items.iter()) {
        let Expression::ImplBlock { ty, methods, .. } = item else {
            continue;
        };
        if !json_enums.iter().any(|name| ty.has_name(name)) {
            continue;
        }
        for method in methods {
            check_method(method, sink);
        }
    }
}

fn check_method(method: &Expression, sink: &LocalSink) {
    let Expression::Function {
        name, name_span, ..
    } = method
    else {
        return;
    };
    if is_reserved_json_method_name(name) {
        sink.push(diagnostics::infer::json_method_override(name, *name_span));
    }
}

fn is_reserved_json_method_name(name: &str) -> bool {
    matches!(name, "MarshalJSON" | "UnmarshalJSON")
}
