use diagnostics::{DiagnosticSink, LisetteDiagnostic};
use semantics::{checker::Checker, lint, pattern_analysis, store::Store};
use syntax::{
    desugar,
    lex::Lexer,
    parse::Parser,
    program::{File, Visibility},
};

use super::init_prelude;

use crate::_harness::register_test_builtins;

use super::TEST_MODULE_ID;

pub fn lint(source: &str) -> Vec<LisetteDiagnostic> {
    let lex_result = Lexer::new(source, 0).lex();
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

    let mut store = Store::new();

    store.add_module(TEST_MODULE_ID);

    let file_id = store.new_file_id();
    store.register_file(file_id, TEST_MODULE_ID);

    let sink = DiagnosticSink::new();

    init_prelude(&mut store);

    let mut checker = Checker::new(&mut store, &sink);
    checker.cursor.module_id = TEST_MODULE_ID.to_string();
    register_test_builtins(&mut checker);
    checker.put_prelude_in_scope();
    checker.register_types_and_values(&ast, &Visibility::Private);

    let mut typed_ast = vec![];

    for expression in ast {
        let type_var = checker.new_type_var();
        let typed_expression = checker.infer_expression(expression, &type_var);
        typed_ast.push(typed_expression);

        if checker.failed() {
            break;
        }
    }

    if !checker.failed() {
        let pattern_ctx =
            pattern_analysis::Context::new(checker.store, &checker.facts.or_pattern_error_spans);
        for expression in &typed_ast {
            pattern_analysis::check(expression, &pattern_ctx, checker.sink);
        }
        checker.facts.pattern_issues = pattern_ctx.take_issues();
    }

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

    checker.store.store_file(TEST_MODULE_ID, typed_file);

    let lint_config = lint::LintConfig::default();
    let module = checker.store.get_module(TEST_MODULE_ID).unwrap();
    let file = module.files.get(&file_id).unwrap();

    let go_package_names = checker.store.go_package_names.clone();
    let lint_ctx = lint::LintContext {
        ast: &file.items,
        facts: &checker.facts,
        module: Some(module),
        config: &lint_config,
        is_d_lis: file.is_d_lis(),
        files: &module.files,
        go_package_names: &go_package_names,
    };
    let lint_sink = DiagnosticSink::new();
    lint::lint_file(&lint_ctx, &lint_sink);
    lint_sink.take()
}
