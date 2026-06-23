use crate::_harness::formatting::{format_diagnostic_unix, format_parse_error_unix};
use crate::_harness::infer::infer;

use diagnostics::LisetteDiagnostic;
use diagnostics::render::{self, Filter};
use syntax::ast::Span;
use syntax::lex::Lexer;
use syntax::parse::Parser;

fn unfiltered() -> Filter {
    Filter {
        errors_only: false,
        warnings_only: false,
    }
}

#[test]
fn unix_parse_error_shape() {
    let source = r#"
fn main() {
  let x = 42;
"#;
    let lex_result = Lexer::new(source, 0).lex();
    let parse_result = Parser::new(lex_result.tokens, source).parse();
    assert!(!parse_result.errors.is_empty());

    let output = format_parse_error_unix(&parse_result.errors[0], source, "src/main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn unix_multi_label_emits_single_location() {
    let source = r#"
fn main() {
  let (x, (y, x)) = (1, (2, 3));
}
"#;
    let result = infer(source);
    assert!(!result.errors.is_empty());

    let output = format_diagnostic_unix(&result.errors[0], source, "src/main.lis");

    assert_eq!(output.lines().count(), 1);

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn unix_diagnostic_without_code_omits_bracket() {
    let source = "let x = 1";
    let span = Span {
        file_id: 0,
        byte_offset: 4,
        byte_length: 1,
    };
    let diagnostic = LisetteDiagnostic::error("Custom message").with_span_label(&span, "here");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(output, "src/main.lis:1:5: error: Custom message");
}

#[test]
fn unix_column_is_byte_offset_within_line() {
    let source = "café x";
    let span = Span {
        file_id: 0,
        byte_offset: 6,
        byte_length: 1,
    };
    let diagnostic = LisetteDiagnostic::error("Bad token").with_span_label(&span, "here");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(output, "src/main.lis:1:7: error: Bad token");
}

#[test]
fn unix_render_emits_only_diagnostic_lines() {
    let source = r#"
fn test() {
  let x: int = "hello";
  let y = unknown_variable;
}
"#;
    let result = infer(source);
    assert!(!result.errors.is_empty());

    let (output, counts) = render::render_unix(
        &result.errors,
        &[],
        |_| None,
        1,
        &unfiltered(),
        source,
        "src/main.lis",
    );

    assert!(!output.contains('\u{1b}'));
    for line in output.lines() {
        assert!(line.starts_with("src/main.lis:"));
        assert!(line.contains(": error: "));
    }
    assert_eq!(output.lines().count() as i32, counts.errors);
}
