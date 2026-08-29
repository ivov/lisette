use diagnostics::{Fix, LisetteDiagnostic, apply_fixes};
use syntax::{lex::Lexer, parse::Parser};

use super::pipeline::{InferredTestFile, TEST_FILE_ID, infer_test_file};

pub fn lint(source: &str) -> Vec<LisetteDiagnostic> {
    let lex_result = Lexer::new(source, TEST_FILE_ID).lex();
    if lex_result.failed() {
        panic!("Lexing failed in lint test: {:?}", lex_result.errors);
    }

    let parse_result = Parser::new(lex_result.tokens, source).parse();
    if parse_result.has_errors() {
        panic!("Parsing failed in lint test: {:?}", parse_result.errors);
    }

    let InferredTestFile { store, checker } = infer_test_file(source, parse_result.ast, &[]);
    let inference_checkpoint = checker.sink.checkpoint();

    passes::run(
        &store,
        &checker.facts,
        &checker.sink,
        passes::LintMode::Run,
        passes::UnusedItemReporting::Report,
    );

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

pub fn apply_infer_fixes(source: &str) -> String {
    let result = super::infer::infer(source);
    let fixes: Vec<&Fix> = result
        .errors
        .iter()
        .filter_map(LisetteDiagnostic::fix)
        .collect();
    assert!(!fixes.is_empty(), "expected at least one fix");
    apply_fixes(source, fixes).source
}

pub fn apply_lint_fixes(source: &str) -> String {
    let lints = lint(source);
    let fixes: Vec<&Fix> = lints.iter().filter_map(LisetteDiagnostic::fix).collect();
    let fixed = apply_fixes(source, fixes).source;

    let reparsed = syntax::build_ast(&fixed, TEST_FILE_ID);
    if !reparsed.errors.is_empty() {
        panic!(
            "Applied fix produced source that no longer parses:\n{fixed}\nerrors: {:?}",
            reparsed.errors
        );
    }

    // A fix that names a shadowed identifier still parses, so reparsing alone
    // cannot catch it.
    let rechecked = super::infer::infer(&fixed);
    if !rechecked.errors.is_empty() {
        panic!(
            "Applied fix produced source that no longer checks:\n{fixed}\nerrors: {:?}",
            rechecked.errors
        );
    }
    fixed
}
