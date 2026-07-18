use diagnostics::{Fix, LisetteDiagnostic, LocalSink, apply_fixes};
use semantics::{checker::TaskState, checker::infer::InferCtx, store::Store};
use stdlib::{Target, get_go_stdlib_typedef};
use syntax::{
    ast::Expression,
    desugar,
    lex::Lexer,
    parse::Parser,
    program::{File, FileImport, UnusedInfo, Visibility},
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

    let locator = deps::TypedefLocator::default();
    let imports: Vec<FileImport> = ast
        .iter()
        .filter_map(|item| {
            let Expression::ModuleImport {
                name,
                name_span,
                alias,
                span,
            } = item
            else {
                return None;
            };
            if let Some(go_pkg) = name.strip_prefix("go:")
                && let Some(typedef) = get_go_stdlib_typedef(go_pkg, Target::host())
            {
                checker.parse_and_register_go_module(&mut store, name, typedef, None, &locator);
            }
            Some(FileImport {
                name: name.clone(),
                name_span: *name_span,
                alias: alias.clone(),
                span: *span,
            })
        })
        .collect();
    checker.put_imported_modules_in_scope(&store, &imports);

    checker.register_types_and_values(&mut store, &ast, &Visibility::Private);
    checker.register_equality(&mut store, &ast);
    checker.finalize_equality(&mut store);
    checker.check_pending_generic_bounds(&store);

    let mut typed_ast = vec![];

    for expression in ast {
        let type_var = checker.new_type_var();
        let typed_expression =
            InferCtx::new(&mut checker, &store).infer_expression(expression, &type_var);
        typed_ast.push(typed_expression);
    }

    {
        let mut ctx = InferCtx::new(&mut checker, &store);
        ctx.resolve_branch_subsumptions();
        ctx.resolve_select_exhaustiveness();
    }

    {
        let folder = semantics::checker::freeze::FreezeFolder::new(&checker.env, &store);
        folder.freeze_facts(&mut checker.facts);
    }
    typed_ast =
        semantics::checker::freeze::FreezeFolder::new(&checker.env, &store).freeze_items(typed_ast);

    let typed_file = File {
        id: file_id,
        module_id: TEST_MODULE_ID.to_string(),
        name: "test.lis".to_string(),
        display_path: "test.lis".to_string(),
        source: source.to_string(),
        items: typed_ast,
        file_comment: None,
    };

    store.store_file(TEST_MODULE_ID, typed_file);
    store.build_closed_domains();

    let inference_len = sink.len();

    let analysis = semantics::context::AnalysisContext::new(&store, &checker.ufcs_methods);
    let mut unused = UnusedInfo::default();
    passes::run(&analysis, &mut checker.facts, &sink, &mut unused, true);

    // Deferred inference errors surface during passes::run, mixed in with
    // the error-severity lint diagnostics the tests assert on.
    let deferred_codes = [
        "infer.statement_as_tail",
        "infer.type_not_inferred",
        "infer.missing_type_argument",
    ];
    let mut all_diagnostics = sink.take();
    let pass_diagnostics = all_diagnostics.split_off(inference_len);
    let mut diagnostics: Vec<LisetteDiagnostic> = all_diagnostics
        .into_iter()
        .filter(|diagnostic| !diagnostic.is_error())
        .collect();
    diagnostics.extend(pass_diagnostics.into_iter().filter(|diagnostic| {
        !diagnostic.is_error()
            || !diagnostic
                .code_str()
                .is_some_and(|code| deferred_codes.contains(&code))
    }));
    diagnostics
}

pub fn apply_lint_fixes(source: &str) -> String {
    let lints = lint(source);
    let fixes: Vec<&Fix> = lints.iter().filter_map(LisetteDiagnostic::fix).collect();
    let fixed = apply_fixes(source, fixes).source;

    let reparsed = syntax::build_ast(&fixed, 0);
    if !reparsed.errors.is_empty() {
        panic!(
            "Applied fix produced source that no longer parses:\n{fixed}\nerrors: {:?}",
            reparsed.errors
        );
    }
    fixed
}
