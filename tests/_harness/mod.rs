pub mod build;
pub mod builders;
pub mod emit;
pub mod filesystem;
pub mod formatting;
pub mod infer;
pub mod lint;
pub mod macros;
pub mod pipeline;
pub mod wrap;

pub use builders::*;
pub use emit::{emit_with_go_typedefs, emit_with_sourcemap};
pub use filesystem::MockFileSystem;
pub use formatting::snapshot_description;
pub use infer::{InferResult, infer, infer_package, infer_with_go_typedefs};

pub const TEST_PACKAGE_ID: &str = "test";

use std::sync::OnceLock;

use diagnostics::LocalSink;
use semantics::prelude::{parse_and_register_prelude, parse_and_register_test_prelude};
use semantics::store::Store;
use syntax::program::{Definition, DefinitionBody, Visibility};
use syntax::types::{CompoundKind, FunctionParameter, Type};

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
        .get_package_mut("prelude")
        .expect("prelude package must exist");

    let mut define = |name: &str, params: Vec<Type>, return_type: Type| {
        package.definitions.insert(
            format!("prelude.{name}").into(),
            Definition {
                visibility: Visibility::Public,
                ty: Type::function(
                    params
                        .into_iter()
                        .map(|ty| FunctionParameter::new(ty, false))
                        .collect(),
                    vec![],
                    Box::new(return_type),
                ),
                name_span: None,
                doc: None,
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
            },
        );
    };

    let unknown = Type::Nominal {
        id: "prelude.Unknown".into(),
        params: vec![],
    };
    let unknown_map = Type::Compound {
        kind: CompoundKind::Map,
        args: vec![string_type(), unknown.clone()],
    };
    let unknown_slice = slice_type(unknown.clone());

    define("get_unknown", vec![], unknown.clone());
    define("takes_unknown", vec![unknown], Type::unit());
    define("get_unknown_map", vec![], unknown_map.clone());
    define("takes_unknown_map", vec![unknown_map], Type::unit());
    define("takes_unknown_slice", vec![unknown_slice], Type::unit());
}
