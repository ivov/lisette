#![no_main]

use libfuzzer_sys::fuzz_target;
use lisette_semantics::checker::infer::InferCtx;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };

    let mut ast_result = lisette_syntax::build_ast(source, 0);
    if ast_result.failed() {
        return;
    }

    let sink = lisette_diagnostics::LocalSink::new();
    let mut store = lisette_semantics::store::Store::new();
    store.add_package("fuzz");
    lisette_semantics::prelude::parse_and_register_prelude(&mut store, &sink);

    let mut checker = lisette_semantics::checker::TaskState::for_package("fuzz");
    checker.put_prelude_in_scope(&store);

    checker.register_types_and_values(
        &mut store,
        &mut ast_result.ast,
        &lisette_syntax::program::Visibility::Private,
    );
    checker.finalize_registration(&mut store);

    let mut ctx = InferCtx::new(&mut checker, &store);
    for expression in ast_result.ast {
        let type_var = ctx.new_type_var();
        let _ = ctx.infer_root_expression(expression, &type_var);

        if ctx.failed() {
            break;
        }
    }
    ctx.resolve_branch_subsumptions();
    ctx.resolve_select_exhaustiveness();
});
