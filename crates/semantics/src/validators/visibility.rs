use diagnostics::LocalSink;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::program::{Definition, DefinitionBody, Visibility};

use crate::store::Store;

pub(super) fn run_module(module_id: &str, store: &Store, sink: &LocalSink) {
    let Some(module) = store.get_module(module_id) else {
        return;
    };

    let module_prefix = format!("{}.", module_id);

    let non_pub_interfaces: HashMap<String, HashSet<String>> = module
        .definitions
        .iter()
        .filter(|(key, _)| key.starts_with(&module_prefix))
        .filter_map(|(_, definition)| {
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
                Some((interface_data.name.to_string(), method_names))
            } else {
                None
            }
        })
        .collect();

    if non_pub_interfaces.is_empty() {
        return;
    }

    for (_, definition) in module
        .definitions
        .iter()
        .filter(|(key, _)| key.starts_with(&module_prefix))
    {
        if let Definition {
            name: Some(name),
            name_span: Some(name_span),
            body: DefinitionBody::Struct { methods, .. },
            ..
        } = definition
        {
            for method_name in methods.keys() {
                for (interface_name, interface_methods) in &non_pub_interfaces {
                    if interface_methods.contains(method_name.as_str()) {
                        let method_key = format!("{}.{}.{}", module_id, name, method_name);
                        let method_is_pub = module
                            .definitions
                            .get(method_key.as_str())
                            .map(|definition| definition.visibility().is_public())
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
