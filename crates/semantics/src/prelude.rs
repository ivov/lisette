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

    let (mut file, errors) = File::from_parse(
        PRELUDE_FILE_ID,
        PRELUDE_PACKAGE_ID,
        "prelude.d.lis".to_string(),
        "prelude.d.lis".to_string(),
        LIS_PRELUDE_SOURCE.to_string(),
        result,
    );
    sink.extend_parse_errors(errors);
    let mut items = file.items.clone();
    file.source_path = deps::prelude_typedef_path();
    file.file_comment = None;
    store.store_file(file);

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

    let (mut file, errors) = File::from_parse(
        file_id,
        TEST_PRELUDE_PACKAGE_ID,
        "test_prelude.d.lis".to_string(),
        "test_prelude.d.lis".to_string(),
        LIS_TEST_PRELUDE_SOURCE.to_string(),
        result,
    );
    sink.extend_parse_errors(errors);
    let mut items = file.items.clone();
    file.file_comment = None;
    store.add_package(TEST_PRELUDE_PACKAGE_ID);
    store.store_file(file);

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
