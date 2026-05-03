use diagnostics::LocalSink;
use stdlib::LIS_PRELUDE_SOURCE;
use syntax::program::{File, Visibility};

use crate::call_classification::compute_module_ufcs;
use crate::checker::{FileContextKind, TaskState};
use crate::store::Store;

pub const PRELUDE_MODULE_ID: &str = "prelude";
pub const PRELUDE_FILE_ID: u32 = 1;

pub fn parse_and_register_prelude(store: &mut Store, sink: &LocalSink) {
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

    let mut checker = TaskState::with_fresh_allocator(sink);
    let module = store
        .get_module(PRELUDE_MODULE_ID)
        .cloned()
        .expect("prelude module must exist");

    checker.with_file_context(
        store,
        PRELUDE_MODULE_ID,
        PRELUDE_FILE_ID,
        &[],
        FileContextKind::Prelude,
        |checker, store| {
            for file in module.all_typedefs() {
                checker.register_type_names(store, &file.items, &Visibility::Public);
            }

            for file in module.all_typedefs() {
                checker.register_type_definitions(store, &file.items);
                checker.register_impl_blocks(store, &file.items);
                checker.register_values(store, &file.items, &Visibility::Public);
            }
        },
    );
}

pub fn compute_prelude_ufcs(store: &Store) -> Vec<(String, String)> {
    let module = store
        .get_module(PRELUDE_MODULE_ID)
        .expect("prelude must exist");
    compute_module_ufcs(module, PRELUDE_MODULE_ID)
}
