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
pub use emit::emit_with_debug_info;
pub use filesystem::MockFileSystem;
pub use infer::{InferResult, infer, infer_module};

pub const TEST_MODULE_ID: &str = "test";

use diagnostics::LocalSink;
use semantics::checker::TaskState;
use semantics::prelude::parse_and_register_prelude;
use semantics::store::Store;
use syntax::program::{Definition, DefinitionBody, Visibility};
use syntax::types::{CompoundKind, Type};

pub fn init_prelude(store: &mut Store) {
    let sink = LocalSink::new();
    parse_and_register_prelude(store, &sink);
}

pub fn register_test_builtins(store: &mut Store, _checker: &mut TaskState) {
    let module = store
        .modules
        .get_mut("prelude")
        .expect("prelude module must exist");

    let mut define = |name: &str, params: Vec<Type>, return_type: Type| {
        let param_mutability = vec![false; params.len()];
        module.definitions.insert(
            format!("prelude.{name}").into(),
            Definition {
                visibility: Visibility::Public,
                ty: Type::Function {
                    params,
                    param_mutability,
                    bounds: vec![],
                    return_type: Box::new(return_type),
                },
                name: None,
                name_span: None,
                doc: None,
                body: DefinitionBody::Value {
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                },
            },
        );
    };

    let unknown = Type::Nominal {
        id: "prelude.Unknown".into(),
        params: vec![],
        underlying_ty: None,
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
