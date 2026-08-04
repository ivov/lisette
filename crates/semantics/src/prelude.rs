use diagnostics::LocalSink;
use stdlib::{LIS_PRELUDE_SOURCE, LIS_TEST_PRELUDE_SOURCE};
use syntax::program::{File, Visibility};

use crate::checker::{FileContext, TaskState};
use crate::store::Store;

pub(crate) const PRELUDE_PACKAGE_ID: &str = "prelude";
pub(crate) const PRELUDE_FILE_ID: u32 = 1;

/// Synthetic, internal package id. The `**` prefix is reserved: imports beginning with
/// it are rejected during package-graph processing, so no user package can collide here.
pub(crate) const TEST_PRELUDE_PACKAGE_ID: &str = "**test_prelude";

pub fn parse_and_register_prelude(store: &mut Store, sink: &LocalSink) {
    let result = syntax::build_ast(LIS_PRELUDE_SOURCE, PRELUDE_FILE_ID);

    sink.extend_parse_errors(result.errors);

    store.store_file(File {
        id: PRELUDE_FILE_ID,
        package_id: PRELUDE_PACKAGE_ID.to_string(),
        name: "prelude.d.lis".to_string(),
        display_path: "prelude.d.lis".to_string(),
        source_path: deps::prelude_typedef_path(),
        source: LIS_PRELUDE_SOURCE.to_string(),
        items: result.ast,
        file_comment: None,
    });

    let mut checker = TaskState::with_fresh_allocator();
    let package = store
        .get_package(PRELUDE_PACKAGE_ID)
        .cloned()
        .expect("prelude package must exist");

    checker.with_file_context_mut(store, FileContext::Prelude, |checker, store| {
        for file in package.typedef_files() {
            checker.register_type_names(store, &file.items, &Visibility::Public);
        }

        for file in package.typedef_files() {
            checker.register_type_definitions(store, &file.items);
            checker.register_impl_blocks(store, &file.items);
            checker.register_values(store, &file.items, &Visibility::Public);
        }
        checker.check_pending_generic_bounds(&*store);
    });
    sink.extend(checker.sink.into_diagnostics());
}

/// Registers the test-only prelude package (`TestContext`). Scopes the main prelude during
/// registration so the signatures resolve, so it must run after the prelude.
pub fn parse_and_register_test_prelude(store: &mut Store, sink: &LocalSink) {
    let file_id = store.new_file_id();
    let result = syntax::build_ast(LIS_TEST_PRELUDE_SOURCE, file_id);

    sink.extend_parse_errors(result.errors);

    store.add_package(TEST_PRELUDE_PACKAGE_ID);
    store.store_file(File {
        id: file_id,
        package_id: TEST_PRELUDE_PACKAGE_ID.to_string(),
        name: "test_prelude.d.lis".to_string(),
        display_path: "test_prelude.d.lis".to_string(),
        source_path: None,
        source: LIS_TEST_PRELUDE_SOURCE.to_string(),
        items: result.ast,
        file_comment: None,
    });

    let mut checker = TaskState::with_fresh_allocator();
    let package = store
        .get_package(TEST_PRELUDE_PACKAGE_ID)
        .cloned()
        .expect("test_prelude package must exist");

    checker.with_file_context_mut(
        store,
        FileContext::TestPrelude { file_id },
        |checker, store| {
            for file in package.typedef_files() {
                checker.register_type_names(store, &file.items, &Visibility::Public);
            }

            for file in package.typedef_files() {
                checker.register_type_definitions(store, &file.items);
                checker.register_impl_blocks(store, &file.items);
                checker.register_values(store, &file.items, &Visibility::Public);
            }
            checker.check_pending_generic_bounds(&*store);
        },
    );
    sink.extend(checker.sink.into_diagnostics());
}
