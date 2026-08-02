use crate::_harness::formatting::{format_diagnostic_unix, format_parse_error_unix};
use crate::_harness::infer::infer;

use diagnostics::render::{self, Filter};
use diagnostics::{Edit, Fix, LisetteDiagnostic};
use syntax::ast::Span;
use syntax::lex::Lexer;
use syntax::parse::Parser;

fn unfiltered() -> Filter {
    Filter::All
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

    assert_eq!(output, "src/main.lis:1:5: error: Custom message: here");
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

    assert_eq!(output, "src/main.lis:1:7: error: Bad token: here");
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
        render::SourceCache::new(|_| None, source, "src/main.lis"),
        1,
        &unfiltered(),
    );

    assert!(!output.contains('\u{1b}'));
    for line in output.lines() {
        assert!(line.starts_with("src/main.lis:"));
        assert!(line.contains(": error: "));
    }
    assert_eq!(output.lines().count(), counts.errors);
}

#[test]
fn unix_flattens_multi_line_help_onto_one_line() {
    let source = "let x = 1";
    let span = Span {
        file_id: 0,
        byte_offset: 4,
        byte_length: 1,
    };
    let diagnostic = LisetteDiagnostic::error("Broken")
        .with_span_label(&span, "here")
        .with_help("First line.\n\n  - second\n  - third");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(output.lines().count(), 1);
    assert_eq!(
        output,
        "src/main.lis:1:5: error: Broken: here · First line. - second - third"
    );
}

#[test]
fn unix_flattens_a_multi_line_message() {
    let source = "let x = 1";
    let diagnostic = LisetteDiagnostic::error("Import cycle detected\n\nalpha -> beta\n──┬──");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(output.lines().count(), 1);
    assert_eq!(output, "error: Import cycle detected alpha -> beta ──┬──");
}

#[test]
fn unix_marks_a_diagnostic_that_carries_a_fix() {
    let source = "let x = 1";
    let span = Span {
        file_id: 0,
        byte_offset: 4,
        byte_length: 1,
    };
    let diagnostic = LisetteDiagnostic::warn("Removable")
        .with_span_label(&span, "drop this")
        .with_resolve_code("removable")
        .with_fix(Fix::new("Remove it", Edit::deletion(span)));

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(
        output,
        "src/main.lis:1:5: warning: Removable: drop this [resolve.removable] [autofixable]"
    );
}

#[test]
fn unix_locates_a_whole_file_diagnostic_at_its_first_column() {
    let diagnostic = LisetteDiagnostic::error("Test file `tests/thing_test.lis` is misnamed")
        .with_resolve_code("wrong_test_file_suffix")
        .with_file_location("tests/thing_test.lis");

    let output = format_diagnostic_unix(&diagnostic, "", "src/main.lis");

    assert_eq!(
        output,
        "tests/thing_test.lis:1:1: error: Test file `tests/thing_test.lis` is misnamed [resolve.wrong_test_file_suffix]"
    );
}

#[test]
fn unix_reports_only_the_start_of_a_span_that_crosses_lines() {
    let source = "fn main() {\n  let x = 1;\n}\n";
    let span = Span {
        file_id: 0,
        byte_offset: 10,
        byte_length: 15,
    };
    let diagnostic = LisetteDiagnostic::error("Spans lines").with_span_label(&span, "block");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(output, "src/main.lis:1:11: error: Spans lines: block");
}

#[test]
fn unix_renders_a_zero_length_span_at_its_position() {
    let source = "let x = 1";
    let span = Span {
        file_id: 0,
        byte_offset: 4,
        byte_length: 0,
    };
    let diagnostic = LisetteDiagnostic::error("Points at a spot").with_span_label(&span, "here");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(output, "src/main.lis:1:5: error: Points at a spot: here");
}

#[test]
fn unix_carries_the_note_alongside_the_help() {
    let source = "let x = 1";
    let span = Span {
        file_id: 0,
        byte_offset: 4,
        byte_length: 1,
    };
    let diagnostic = LisetteDiagnostic::error("Invalid or-pattern")
        .with_span_label(&span, "here")
        .with_resolve_code("or_pattern")
        .with_help("Use a single binding")
        .with_note("Or-patterns can only be used in `match`, `if let`, and `while let`.");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert_eq!(
        output,
        "src/main.lis:1:5: error: Invalid or-pattern: here · Use a single binding Or-patterns can only be used in `match`, `if let`, and `while let` [resolve.or_pattern]"
    );
}

#[test]
fn unix_flattens_a_bare_carriage_return() {
    let source = "let x = 1";
    let diagnostic = LisetteDiagnostic::error("Captured output:\roverwritten");

    let output = format_diagnostic_unix(&diagnostic, source, "src/main.lis");

    assert!(!output.contains('\r'));
    assert_eq!(output, "error: Captured output: overwritten");
}

#[test]
fn unix_flattens_a_newline_inside_a_filename() {
    let source = "let x = 1";
    let span = Span {
        file_id: 0,
        byte_offset: 4,
        byte_length: 1,
    };
    let diagnostic = LisetteDiagnostic::error("Broken").with_span_label(&span, "here");

    let output = format_diagnostic_unix(&diagnostic, source, "src/inject\nevil.lis");

    assert_eq!(output.lines().count(), 1);
    assert_eq!(output, "src/inject evil.lis:1:5: error: Broken: here");
}

#[test]
fn unix_flattens_a_newline_inside_a_file_location() {
    let diagnostic = LisetteDiagnostic::error("Misnamed file")
        .with_resolve_code("wrong_test_file_suffix")
        .with_file_location("tests/inject\nevil.lis");

    let output = format_diagnostic_unix(&diagnostic, "", "src/main.lis");

    assert_eq!(output.lines().count(), 1);
    assert_eq!(
        output,
        "tests/inject evil.lis:1:1: error: Misnamed file [resolve.wrong_test_file_suffix]"
    );
}
