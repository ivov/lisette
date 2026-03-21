#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };

    let ast_result = lisette_syntax::build_ast(source, 0);
    if ast_result.failed() {
        return;
    }

    let desugar_result = lisette_syntax::desugar::desugar(ast_result.ast);
    if !desugar_result.errors.is_empty() {
        return;
    }

    let sink = lisette_diagnostics::DiagnosticSink::new();
    let mut store = lisette_semantics::store::Store::new();
    store.add_module("fuzz");
    lisette_semantics::prelude::parse_and_register_prelude(&mut store, &sink);

    let mut checker = lisette_semantics::checker::Checker::new(&mut store, &sink);
    checker
        .ufcs_methods
        .extend(lisette_semantics::prelude::compute_prelude_ufcs(checker.store));
    checker.cursor.module_id = "fuzz".to_string();
    checker.put_prelude_in_scope();

    for expression in desugar_result.ast {
        let type_var = checker.new_type_var();
        let _ = checker.infer_expression(expression, &type_var);

        if checker.failed() {
            break;
        }
    }
});
