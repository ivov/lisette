#![no_main]

use libfuzzer_sys::fuzz_target;
use lisette_syntax::ast::Expression;
use lisette_syntax::imports::scan_imports;
use lisette_syntax::program::FileImport;

/// The graph reads imports with the scanner, which must agree with the parser.
fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };

    // Scanned first for panic coverage on the inputs the parser rejects, which
    // discard their AST and so have nothing to compare against.
    let scanned = scan_imports(source, 0);

    let result = lisette_syntax::build_ast(source, 0);
    if result.failed() {
        return;
    }

    let parsed: Vec<FileImport> = result
        .ast
        .iter()
        .filter_map(|item| match item {
            Expression::ModuleImport {
                name,
                name_span,
                alias,
                span,
            } => Some(FileImport {
                name: name.clone(),
                name_span: *name_span,
                alias: alias.clone(),
                span: *span,
            }),
            _ => None,
        })
        .collect();

    assert_eq!(scanned, parsed, "scanner and parser disagree on: {source:?}");
});
