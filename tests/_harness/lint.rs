use diagnostics::{LisetteDiagnostic, LocalSink};
use semantics::{checker::TaskState, store::Store, validators};
use syntax::{
    desugar,
    lex::Lexer,
    parse::Parser,
    program::{File, UnusedInfo, Visibility},
};

use super::init_prelude;

use crate::_harness::register_test_builtins;

use super::TEST_MODULE_ID;

pub fn lint(source: &str) -> Vec<LisetteDiagnostic> {
    let mut store = Store::new();
    store.add_module(TEST_MODULE_ID);

    let sink = LocalSink::new();

    init_prelude(&mut store);

    // Parser::new hardcodes file_id=0 in spans, so pin the test file to that id
    // too; source-based diagnostics rely on span.file_id matching files map key.
    let file_id = 0u32;
    store.register_file(file_id, TEST_MODULE_ID);

    let lex_result = Lexer::new(source, file_id).lex();
    if lex_result.failed() {
        panic!("Lexing failed in lint test: {:?}", lex_result.errors);
    }

    let parse_result = Parser::new(lex_result.tokens, source).parse();
    if parse_result.failed() {
        panic!("Parsing failed in lint test: {:?}", parse_result.errors);
    }

    let desugar_result = desugar::desugar(parse_result.ast);
    if !desugar_result.errors.is_empty() {
        panic!(
            "Desugaring failed in lint test: {:?}",
            desugar_result.errors
        );
    }
    let ast = desugar_result.ast;

    let mut checker = TaskState::with_fresh_allocator(&sink);
    checker.cursor.module_id = TEST_MODULE_ID.to_string();
    register_test_builtins(&mut store, &mut checker);
    checker.put_prelude_in_scope(&store);
    checker.register_types_and_values(&mut store, &ast, &Visibility::Private);

    let mut typed_ast = vec![];

    for expression in ast {
        let type_var = checker.new_type_var();
        let typed_expression = checker.infer_expression(&mut store, expression, &type_var);
        typed_ast.push(typed_expression);

        if checker.failed() {
            break;
        }
    }

    {
        let folder = semantics::checker::freeze::FreezeFolder::new(&checker.env);
        folder.freeze_facts(&mut checker.facts);
    }
    typed_ast = semantics::checker::freeze::FreezeFolder::new(&checker.env).freeze_items(typed_ast);

    if checker.failed() {
        return vec![];
    }

    let typed_file = File {
        id: file_id,
        module_id: TEST_MODULE_ID.to_string(),
        name: "test.lis".to_string(),
        source: source.to_string(),
        items: typed_ast,
    };

    store.store_file(TEST_MODULE_ID, typed_file);

    let analysis = semantics::context::AnalysisContext::new(&store, &checker.ufcs_methods);
    let mut unused = UnusedInfo::default();
    validators::run(&analysis, &mut checker.facts, &sink, &mut unused, true);

    sink.take()
}
