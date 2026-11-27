pub mod build;
pub mod builders;
pub mod emit;
pub mod filesystem;
pub mod formatting;
#[allow(dead_code)]
pub mod go;
#[allow(dead_code)]
pub mod go_types;
pub mod infer;
pub mod lint;
#[allow(dead_code)]
pub mod lists;
pub mod macros;
#[allow(dead_code)]
pub mod markdown;
pub mod pipeline;
pub mod wrap;

pub use builders::*;
pub use emit::{emit_with_go_typedefs, emit_with_sourcemap};
pub use filesystem::MockFileSystem;
pub use formatting::{snapshot_description, syntax_tree_snapshot};
pub use infer::{InferResult, infer, infer_package, infer_with_go_typedefs};
use syntax::program::{PRELUDE_PACKAGE_ID, PRELUDE_PREFIX, ValueKind};

pub const TEST_PACKAGE_ID: &str = "test";

use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use diagnostics::LocalSink;
use semantics::prelude::{parse_and_register_prelude, parse_and_register_test_prelude};
use semantics::store::Store;
use syntax::program::{Definition, DefinitionBody, Visibility};
use syntax::types::{CompoundKind, FunctionParameter, Symbol, Type};

pub fn new_test_store() -> Store {
    static TEMPLATE: OnceLock<Store> = OnceLock::new();

    TEMPLATE
        .get_or_init(|| {
            let mut store = Store::new();
            let sink = LocalSink::new();
            parse_and_register_prelude(&mut store, &sink);
            parse_and_register_test_prelude(&mut store, &sink);
            register_test_builtins(&mut store);
            store
        })
        .clone()
}

fn register_test_builtins(store: &mut Store) {
    let package = store
        .edit_package(PRELUDE_PACKAGE_ID)
        .expect("prelude package must exist");

    let mut define = |name: &str, params: Vec<Type>, return_type: Type| {
        package.definitions.insert(
            Symbol::from_raw(format!("{PRELUDE_PREFIX}{name}")),
            Definition {
                visibility: Visibility::Public,
                ty: Type::function(
                    params.into_iter().map(FunctionParameter::new).collect(),
                    vec![],
                    Box::new(return_type),
                ),
                name_span: None,
                doc: None,
                body: DefinitionBody::Value {
                    kind: ValueKind::Runtime,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                    go_type_terms: vec![],
                    superseded_by: None,
                    impl_receiver: None,
                },
            },
        );
    };

    let unknown = Type::Nominal {
        id: Symbol::from_raw("prelude.Unknown"),
        params: vec![],
        writable: false,
    };
    let unknown_map = Type::compound(CompoundKind::Map, vec![string_type(), unknown.clone()]);
    let unknown_slice = slice_type(unknown.clone());

    define("get_unknown", vec![], unknown.clone());
    define("takes_unknown", vec![unknown], Type::unit());
    define("get_unknown_map", vec![], unknown_map.clone());
    define("takes_unknown_map", vec![unknown_map], Type::unit());
    define("takes_unknown_slice", vec![unknown_slice], Type::unit());
}

pub fn assert_completes_within(timeout: Duration, check: impl FnOnce()) {
    use std::process::{Command, Stdio};
    use std::time::Instant;
    use std::{env, fs, thread};

    let current_thread = thread::current();
    let name = current_thread.name().expect("test thread has a name");
    if env::var("LISETTE_SUBPROCESS_TEST").as_deref() == Ok(name) {
        check();
        return;
    }
    let output = tempfile::NamedTempFile::new().expect("create child output");
    let mut child = Command::new(env::current_exe().expect("locate test binary"))
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("LISETTE_SUBPROCESS_TEST", name)
        .stdout(Stdio::from(
            output.as_file().try_clone().expect("open stdout"),
        ))
        .stderr(Stdio::from(
            output.as_file().try_clone().expect("open stderr"),
        ))
        .spawn()
        .expect("start isolated regression");
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll isolated regression") {
            break status;
        }
        if start.elapsed() >= timeout {
            child.kill().expect("kill timed-out regression");
            child.wait().expect("reap timed-out regression");
            panic!(
                "{name} exceeded {timeout:?}:\n{}",
                fs::read_to_string(output.path()).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(10));
    };
    let output = fs::read_to_string(output.path()).expect("read child output");
    assert!(status.success(), "{name} failed: {status}\n{output}");
    assert!(
        output.contains("1 passed;"),
        "child did not run the regression:\n{output}"
    );
}

/// The `lis` binary that `cargo build -p lisette` writes.
#[allow(dead_code)]
pub fn lis_binary() -> PathBuf {
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../target"));
    let binary = target.join("debug/lis");
    assert!(
        binary.exists(),
        "{} not found: run `cargo build -p lisette` first",
        binary.display()
    );
    binary
}
