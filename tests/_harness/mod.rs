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
use syntax::program::{Definition, Visibility};
use syntax::types::Type;

pub fn init_prelude(store: &mut Store) {
    let sink = LocalSink::new();
    parse_and_register_prelude(store, &sink);
}

pub fn register_test_builtins(store: &mut Store, _checker: &mut TaskState) {
    let module_id = "prelude";
    let module = store
        .modules
        .get_mut(module_id)
        .expect("prelude module must exist");

    let unknown_type = Type::Nominal {
        id: "prelude.Unknown".into(),
        params: vec![],
        underlying_ty: None,
    };
    let get_unknown_ty = Type::Function {
        params: vec![],
        param_mutability: vec![],
        bounds: vec![],
        return_type: Box::new(unknown_type.clone()),
    };
    module.definitions.insert(
        "prelude.get_unknown".into(),
        Definition::Value {
            visibility: Visibility::Public,
            ty: get_unknown_ty,
            name_span: None,
            allowed_lints: vec![],
            go_hints: vec![],
            go_name: None,
            doc: None,
        },
    );

    let takes_unknown_ty = Type::Function {
        params: vec![unknown_type],
        param_mutability: vec![false],
        bounds: vec![],
        return_type: Box::new(Type::unit()),
    };
    module.definitions.insert(
        "prelude.takes_unknown".into(),
        Definition::Value {
            visibility: Visibility::Public,
            ty: takes_unknown_ty,
            name_span: None,
            allowed_lints: vec![],
            go_hints: vec![],
            go_name: None,
            doc: None,
        },
    );
}
