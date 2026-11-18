#![no_main]

use libfuzzer_sys::fuzz_target;
use lisette_semantics::checker::TaskState;
use lisette_semantics::store::Store;
use lisette_syntax::program::File;
use std::sync::OnceLock;

const PACKAGE_ID: &str = "fuzz";
const FILE_ID: u32 = 0;

fn prelude_store() -> Store {
    static TEMPLATE: OnceLock<Store> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| {
            let mut store = Store::new();
            let sink = lisette_diagnostics::LocalSink::new();
            lisette_semantics::prelude::parse_and_register_prelude(&mut store, &sink);
            store
        })
        .clone()
}

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };

    let ast_result = lisette_syntax::build_ast(source, FILE_ID);
    if ast_result.has_errors() {
        return;
    }

    let mut store = prelude_store();
    store.add_package(PACKAGE_ID);
    let (file, _) = File::from_parse(
        FILE_ID,
        PACKAGE_ID,
        "fuzz.lis".to_string(),
        "fuzz.lis".to_string(),
        source.to_string(),
        ast_result,
    );
    store.store_file(file);

    let mut checker = TaskState::for_package(PACKAGE_ID);
    let package = checker.register_package(&mut store, PACKAGE_ID);
    checker.finalize_registration(&mut store);
    checker.infer_package(&mut store, package);
    checker.check_post_inference_bounds(&store);
});
