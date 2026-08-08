use diagnostics::{Fix, LisetteDiagnostic, apply_fixes};
use semantics::{checker::TaskState, checker::infer::InferCtx};
use stdlib::{Target, get_go_stdlib_typedef};
use syntax::{
    ast::Expression,
    lex::Lexer,
    parse::Parser,
    program::{File, FileImport, Visibility},
};

use super::new_test_store;

use super::TEST_PACKAGE_ID;

pub fn lint(source: &str) -> Vec<LisetteDiagnostic> {
    let mut store = new_test_store();
    store.add_package(TEST_PACKAGE_ID);

    // Parser::new hardcodes file_id=0 in spans, so pin the test file to that id too.
    let file_id = 0u32;

    let lex_result = Lexer::new(source, file_id).lex();
    if lex_result.failed() {
        panic!("Lexing failed in lint test: {:?}", lex_result.errors);
    }

    let parse_result = Parser::new(lex_result.tokens, source).parse();
    if parse_result.failed() {
        panic!("Parsing failed in lint test: {:?}", parse_result.errors);
    }

    let mut ast = parse_result.ast;

    let mut checker = TaskState::for_package(TEST_PACKAGE_ID);
    checker.put_prelude_in_scope(&store);

    let locator = deps::TypedefLocator::default();
    let imports: Vec<FileImport> = ast
        .iter()
        .filter_map(|item| {
            let Expression::PackageImport {
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
                checker.parse_and_register_go_package(&mut store, name, typedef, None, &locator);
            }
            Some(FileImport {
                name: name.clone(),
                name_span: *name_span,
                alias: alias.clone(),
                span: *span,
            })
        })
        .collect();
    checker.put_imported_packages_in_scope(&store, &imports);

    checker.register_types_and_values(&mut store, &mut ast, &Visibility::Private);
    checker.finalize_registration(&mut store);

    let mut typed_ast = vec![];
    {
        let mut ctx = InferCtx::new(&mut checker, &store);
        for expression in ast {
            let type_var = ctx.new_type_var();
            let typed_expression = ctx.infer_root_expression(expression, &type_var);
            typed_ast.push(typed_expression);
        }

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
        package_id: TEST_PACKAGE_ID.to_string(),
        name: "test.lis".to_string(),
        display_path: "test.lis".to_string(),
        source_path: None,
        source: source.to_string(),
        items: typed_ast,
        file_comment: None,
    };

    store.store_file(typed_file);
    let inference_checkpoint = checker.sink.checkpoint();

    passes::run(&store, &checker.facts, &checker.sink, passes::LintMode::Run);

    // Deferred inference errors surface during passes::run, mixed in with
    // the error-severity lint diagnostics the tests assert on.
    let deferred_codes = [
        "infer.statement_as_tail",
        "infer.type_not_inferred",
        "infer.missing_type_argument",
    ];
    let (inference_diagnostics, pass_diagnostics) =
        checker.sink.into_diagnostics_since(inference_checkpoint);
    let mut diagnostics: Vec<LisetteDiagnostic> = inference_diagnostics
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
