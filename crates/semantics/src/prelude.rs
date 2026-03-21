use diagnostics::DiagnosticSink;
use stdlib::LIS_PRELUDE_SOURCE;
use syntax::program::{Definition, File, Visibility};

use crate::call_classification::compute_module_ufcs;
use crate::checker::Checker;
use crate::store::Store;

pub const PRELUDE_MODULE_ID: &str = "prelude";
pub const PRELUDE_FILE_ID: u32 = 1;

pub fn parse_and_register_prelude(store: &mut Store, sink: &DiagnosticSink) {
    let result = syntax::build_ast(LIS_PRELUDE_SOURCE, PRELUDE_FILE_ID);

    sink.extend_parse_errors(result.errors);

    store.mark_visited(PRELUDE_MODULE_ID);
    store.store_file(
        PRELUDE_MODULE_ID,
        File {
            id: PRELUDE_FILE_ID,
            module_id: PRELUDE_MODULE_ID.to_string(),
            name: "prelude.d.lis".to_string(),
            source: LIS_PRELUDE_SOURCE.to_string(),
            items: result.ast,
        },
    );

    let mut checker = Checker::new(store, sink);
    checker.cursor.module_id = PRELUDE_MODULE_ID.to_string();
    checker.cursor.file_id = Some(PRELUDE_FILE_ID);

    let module = checker
        .store
        .get_module(PRELUDE_MODULE_ID)
        .cloned()
        .expect("prelude module must exist");

    for file in module.all_typedefs() {
        checker.register_type_names(&file.items, &Visibility::Public);
    }

    checker.reset_scopes();
    checker.put_unprefixed_module_in_scope(PRELUDE_MODULE_ID);

    for file in module.all_typedefs() {
        checker.register_types(&file.items);
        checker.register_values(&file.items, &Visibility::Public);
    }

    checker.cursor.file_id = None;

    let module = checker
        .store
        .get_module_mut(PRELUDE_MODULE_ID)
        .expect("prelude module must exist");

    let short_forms: Vec<_> = module
        .definitions
        .iter()
        .filter_map(|(name, definition)| {
            let Definition::Enum { variants, .. } = definition else {
                return None;
            };
            let enum_name = name.rsplit('.').next().unwrap();
            Some(
                variants
                    .iter()
                    .map(|v| {
                        let short = format!("prelude.{}", v.name);
                        let full = format!("prelude.{}.{}", enum_name, v.name);
                        (short, full)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect();

    for (short, full) in short_forms {
        let definition = module
            .definitions
            .get(full.as_str())
            .cloned()
            .expect("enum variant definition must exist for qualified name");
        module.definitions.entry(short.into()).or_insert(definition);
    }
}

pub fn compute_prelude_ufcs(store: &Store) -> Vec<(String, String)> {
    let module = store
        .get_module(PRELUDE_MODULE_ID)
        .expect("prelude must exist");
    compute_module_ufcs(module, PRELUDE_MODULE_ID)
}
