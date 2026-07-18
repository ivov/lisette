use diagnostics::LocalSink;
use stdlib::{LIS_PRELUDE_SOURCE, LIS_TEST_PRELUDE_SOURCE};
use syntax::program::{File, Visibility};

use crate::call_classification::compute_module_ufcs;
use crate::checker::{FileContextKind, TaskState};
use crate::store::Store;

pub const PRELUDE_MODULE_ID: &str = "prelude";
pub const PRELUDE_FILE_ID: u32 = 1;

/// Synthetic, internal module id. The `**` prefix is reserved: imports beginning with
/// it are rejected during module-graph processing, so no user module can collide here.
pub const TEST_PRELUDE_MODULE_ID: &str = "**test_prelude";

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
            display_path: "prelude.d.lis".to_string(),
            source: LIS_PRELUDE_SOURCE.to_string(),
            items: result.ast,
            file_comment: None,
        },
    );

    if let Some(path) = deps::prelude_typedef_path() {
        store.typedef_paths.insert(PRELUDE_FILE_ID, path);
    }

    let mut checker = TaskState::with_fresh_allocator(sink);
    let module = store
        .get_module(PRELUDE_MODULE_ID)
        .cloned()
        .expect("prelude module must exist");

    checker.with_file_context_mut(
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
            checker.check_pending_generic_bounds(&*store);
        },
    );
}

/// Registers the test-only prelude module (`TestContext`). Scopes the main prelude during
/// registration so the signatures resolve, so it must run after the prelude.
pub fn parse_and_register_test_prelude(store: &mut Store, sink: &LocalSink) {
    let file_id = store.new_file_id();
    let result = syntax::build_ast(LIS_TEST_PRELUDE_SOURCE, file_id);

    sink.extend_parse_errors(result.errors);

    store.mark_visited(TEST_PRELUDE_MODULE_ID);
    store.add_module(TEST_PRELUDE_MODULE_ID);
    store.store_file(
        TEST_PRELUDE_MODULE_ID,
        File {
            id: file_id,
            module_id: TEST_PRELUDE_MODULE_ID.to_string(),
            name: "test_prelude.d.lis".to_string(),
            display_path: "test_prelude.d.lis".to_string(),
            source: LIS_TEST_PRELUDE_SOURCE.to_string(),
            items: result.ast,
            file_comment: None,
        },
    );

    let mut checker = TaskState::with_fresh_allocator(sink);
    let module = store
        .get_module(TEST_PRELUDE_MODULE_ID)
        .cloned()
        .expect("test_prelude module must exist");

    checker.with_file_context_mut(
        store,
        TEST_PRELUDE_MODULE_ID,
        file_id,
        &[],
        FileContextKind::TestPrelude,
        |checker, store| {
            for file in module.all_typedefs() {
                checker.register_type_names(store, &file.items, &Visibility::Public);
            }

            for file in module.all_typedefs() {
                checker.register_type_definitions(store, &file.items);
                checker.register_impl_blocks(store, &file.items);
                checker.register_values(store, &file.items, &Visibility::Public);
            }
            checker.check_pending_generic_bounds(&*store);
        },
    );
}

pub fn compute_prelude_ufcs(store: &Store) -> Vec<(String, String)> {
    let module = store
        .get_module(PRELUDE_MODULE_ID)
        .expect("prelude must exist");
    compute_module_ufcs(module)
}
