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
    let mut items = result.ast;

    store.store_file(File {
        id: PRELUDE_FILE_ID,
        package_id: PRELUDE_PACKAGE_ID.to_string(),
        parse_status: result.status,
        name: "prelude.d.lis".to_string(),
        display_path: "prelude.d.lis".to_string(),
        source_path: deps::prelude_typedef_path(),
        source: LIS_PRELUDE_SOURCE.to_string(),
        items: items.clone(),
        file_comment: None,
    });

    let mut checker = TaskState::for_package(PRELUDE_PACKAGE_ID);
    checker.with_file_context_mut(store, FileContext::Prelude, |checker, store| {
        checker.register_types_and_values(store, &mut items, &Visibility::Public);
        checker.finalize_registration(store);
    });
    sink.extend(checker.sink.into_diagnostics());
}

/// Registers the test-only prelude package (`TestContext`). Scopes the main prelude during
/// registration so the signatures resolve, so it must run after the prelude.
pub fn parse_and_register_test_prelude(store: &mut Store, sink: &LocalSink) {
    let file_id = store.new_file_id();
    let result = syntax::build_ast(LIS_TEST_PRELUDE_SOURCE, file_id);

    sink.extend_parse_errors(result.errors);
    let mut items = result.ast;

    store.add_package(TEST_PRELUDE_PACKAGE_ID);
    store.store_file(File {
        id: file_id,
        package_id: TEST_PRELUDE_PACKAGE_ID.to_string(),
        parse_status: result.status,
        name: "test_prelude.d.lis".to_string(),
        display_path: "test_prelude.d.lis".to_string(),
        source_path: None,
        source: LIS_TEST_PRELUDE_SOURCE.to_string(),
        items: items.clone(),
        file_comment: None,
    });

    let mut checker = TaskState::for_package(TEST_PRELUDE_PACKAGE_ID);
    checker.with_file_context_mut(
        store,
        FileContext::TestPrelude { file_id },
        |checker, store| {
            checker.register_types_and_values(store, &mut items, &Visibility::Public);
            checker.finalize_registration(store);
        },
    );
    sink.extend(checker.sink.into_diagnostics());
}
