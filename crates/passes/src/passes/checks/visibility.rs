use diagnostics::LocalSink;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::program::{Definition, DefinitionBody, Visibility};

use semantics::store::Store;

pub(crate) fn run_package(package_id: &str, store: &Store, sink: &LocalSink) {
    let Some(package) = store.get_package(package_id) else {
        return;
    };

    let package_prefix = format!("{}.", package_id);

    let non_pub_interfaces: HashMap<String, HashSet<String>> = package
        .definitions
        .iter()
        .filter(|(key, _)| key.starts_with(&package_prefix))
        .filter(|(_, definition)| !store.is_test_definition(definition))
        .filter_map(|(qualified_name, definition)| {
            if let Definition {
                visibility: Visibility::Private,
                body:
                    DefinitionBody::Interface {
                        definition: interface_data,
                    },
                ..
            } = definition
            {
                let method_names = interface_data
                    .methods
                    .keys()
                    .map(|k| k.to_string())
                    .collect();
                Some((qualified_name.last_segment().to_string(), method_names))
            } else {
                None
            }
        })
        .collect();

    if non_pub_interfaces.is_empty() {
        return;
    }

    for (key, definition) in package
        .definitions
        .iter()
        .filter(|(key, _)| key.starts_with(&package_prefix))
        .filter(|(_, definition)| !store.is_test_definition(definition))
    {
        if let Definition {
            name_span: Some(name_span),
            body: DefinitionBody::Struct { methods, .. },
            ..
        } = definition
        {
            let name = key.last_segment();
            for method_name in methods.keys() {
                for (interface_name, interface_methods) in &non_pub_interfaces {
                    if interface_methods.contains(method_name.as_str()) {
                        let method_key = format!("{}.{}.{}", package_id, name, method_name);
                        let method_is_pub = package
                            .definitions
                            .get(method_key.as_str())
                            .map(|definition| definition.visibility.is_public())
                            .unwrap_or(false);

                        if method_is_pub {
                            sink.push(diagnostics::infer::non_pub_interface_with_pub_impl(
                                interface_name,
                                name,
                                *name_span,
                            ));
                            return;
                        }
                    }
                }
            }
        }
    }
}
