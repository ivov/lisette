use diagnostics::LocalSink;
use syntax::FileParseStatus;
use syntax::ast::{Annotation, Expression, Visibility};

use crate::store::{ENTRY_FILE_ID, ENTRY_PACKAGE_ID, Store};

pub(super) fn check_entry_function(store: &Store, requires_main: bool, sink: &LocalSink) {
    let Some(package) = store.get_package(ENTRY_PACKAGE_ID) else {
        return;
    };
    let mut has_main = false;
    for file in package.source_files().filter(|file| !file.is_test()) {
        for item in &file.items {
            let Expression::Function {
                name,
                name_span,
                generics,
                params,
                return_annotation,
                visibility,
                ..
            } = item
            else {
                continue;
            };
            if name != "main" {
                continue;
            }
            has_main = true;
            if !generics.is_empty()
                || !params.is_empty()
                || *return_annotation != Annotation::Unknown
                || *visibility == Visibility::Public
            {
                sink.push(diagnostics::infer::invalid_main_signature(*name_span));
            }
        }
    }

    let parsed = package
        .files
        .values()
        .all(|file| file.parse_status == FileParseStatus::Clean);
    if requires_main
        && !has_main
        && parsed
        && let Some(entry_file) = store.get_file(ENTRY_FILE_ID)
        && !entry_file.is_test()
    {
        sink.push(diagnostics::infer::missing_main_function(
            &entry_file.display_path,
        ));
    }
}
