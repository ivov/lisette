use crate::assert_lex_snapshot;

#[test]
fn assignment() {
    let input = "let a = 123;";
    assert_lex_snapshot!(input);
}

#[test]
fn boolean_true() {
    let input = "true";
    assert_lex_snapshot!(input);
}

#[test]
fn boolean_false() {
    let input = "false";
    assert_lex_snapshot!(input);
}

#[test]
fn chaining_calls_with_newline() {
    let input = "method1()
            .method2()";
    assert_lex_snapshot!(input);
}

#[test]
fn chaining_methods_with_question_mark() {
    let input = "method1()?
            .method2()";
    assert_lex_snapshot!(input);
}

#[test]
fn chaining_methods_with_comment() {
    let input = "method1()
            // comment
            .method2()";
    assert_lex_snapshot!(input);
}

#[test]
fn else_on_next_line() {
    let input = "if x { 1 }
    else { 2 }";
    assert_lex_snapshot!(input);
}

#[test]
fn brace_on_next_line() {
    let input = "fn test()
{
    1
}";
    assert_lex_snapshot!(input);
}

#[test]
fn no_semicolon_before_closing_brace() {
    let input = "struct Box<T> {
    value: T
}";
    assert_lex_snapshot!(input);
}

#[test]
fn char_letter() {
    let input = "\'b\'";
    assert_lex_snapshot!(input);
}

#[test]
fn char_escaped_newline() {
    let input = "'\\n'";
    assert_lex_snapshot!(input);
}

#[test]
fn char_escaped_tab() {
    let input = "'\\t'";
    assert_lex_snapshot!(input);
}

#[test]
fn char_escaped_backslash() {
    let input = "'\\\\'";
    assert_lex_snapshot!(input);
}

#[test]
fn char_escaped_quote() {
    let input = "'\\''";
    assert_lex_snapshot!(input);
}

#[test]
fn comment() {
    let input = "42; // meaning of life";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_empty() {
    let input = "//";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_only() {
    let input = "// This is just a comment";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_consecutive() {
    let input = "42;\n// first comment\n84;\n// second comment";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_with_symbols() {
    let input = "// comment with symbols: !@#$%^&*()_+-=[]{}|;:'\",.<>/?";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_with_slashes() {
    let input = "// comment with // multiple slashes";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_with_keywords() {
    let input = "// if else while true false 42";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_vs_division() {
    let input = "10 / 2; // division before comment";
    assert_lex_snapshot!(input);
}

#[test]
fn comment_adjacent() {
    let input = "// first line\n// second line";
    assert_lex_snapshot!(input);
}

#[test]
fn doc_comment() {
    let input = "/// This is a doc comment";
    assert_lex_snapshot!(input);
}

#[test]
fn doc_comment_before_fn() {
    let input = "/// Adds two numbers\nfn add(a, b) { a + b }";
    assert_lex_snapshot!(input);
}

#[test]
fn doc_comment_multiline() {
    let input = "/// First line\n/// Second line\nfn foo() {}";
    assert_lex_snapshot!(input);
}

#[test]
fn doc_comment_vs_comment() {
    let input = "// regular comment\n/// doc comment";
    assert_lex_snapshot!(input);
}

#[test]
fn doc_comment_four_slashes_is_comment() {
    let input = "//// This is a divider, not a doc comment";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_simple() {
    let input = "f\"hello world\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_variable() {
    let input = "f\"hello {name}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_field_access() {
    let input = "f\"name: {person.name}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_multiple_interpolations() {
    let input = "f\"hello {first} {last}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_escaped_braces() {
    let input = "f\"literal {{ and }} braces\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_empty() {
    let input = "f\"\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_addition() {
    let input = "f\"sum: {x + y}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_function_call() {
    let input = "f\"result: {foo()}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_method_call() {
    let input = "f\"result: {obj.method()}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_index_access() {
    let input = "f\"result: {arr[0]}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_map_access() {
    let input = r#"f"result: {map["key"]}""#;
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_if_expression() {
    let input = "f\"result: {if x { 1 } else { 0 }}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_struct_literal() {
    let input = "f\"point: {Point { x: 1, y: 2 }}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_char_brace() {
    let input = "f\"result: {c == '{'}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_nested_fstring() {
    let input = r#"f"outer: {f"inner: {x}"}""#;
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_closing_brace_in_string() {
    let input = r#"f"val: {map["}"]}""#;
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_closing_brace_in_char() {
    let input = "f\"val: {c == '}'}\"";
    assert_lex_snapshot!(input);
}

#[test]
fn format_string_with_function_call_string_arg() {
    let input = r#"f"x: {func("arg")}""#;
    assert_lex_snapshot!(input);
}

#[test]
fn identifier() {
    let input = "hello";
    assert_lex_snapshot!(input);
}

#[test]
fn integer() {
    let input = "1";
    assert_lex_snapshot!(input);
}

#[test]
fn float() {
    let input = "1.68";
    assert_lex_snapshot!(input);
}

#[test]
fn integer_with_underscore_separators() {
    let input = "1_000_000";
    assert_lex_snapshot!(input);
}

#[test]
fn float_with_underscore_separators() {
    let input = "1_000.123_456";
    assert_lex_snapshot!(input);
}

#[test]
fn hex_basic() {
    let input = "0xFF";
    assert_lex_snapshot!(input);
}

#[test]
fn hex_uppercase_prefix() {
    let input = "0XFF";
    assert_lex_snapshot!(input);
}

#[test]
fn hex_mixed_case_digits() {
    let input = "0xDeAdBeEf";
    assert_lex_snapshot!(input);
}

#[test]
fn hex_with_underscores() {
    let input = "0xFF_FF_FF_FF";
    assert_lex_snapshot!(input);
}

#[test]
fn hex_long() {
    let input = "0xC96C5795D7870F42";
    assert_lex_snapshot!(input);
}

#[test]
fn scientific_basic() {
    let input = "1e10";
    assert_lex_snapshot!(input);
}

#[test]
fn scientific_uppercase_e() {
    let input = "1E10";
    assert_lex_snapshot!(input);
}

#[test]
fn scientific_positive_exponent() {
    let input = "1.5e+10";
    assert_lex_snapshot!(input);
}

#[test]
fn scientific_negative_exponent() {
    let input = "1.5e-10";
    assert_lex_snapshot!(input);
}

#[test]
fn scientific_large_exponent() {
    let input = "1.7976931348623157e+308";
    assert_lex_snapshot!(input);
}

#[test]
fn scientific_with_underscore() {
    let input = "1_000e10";
    assert_lex_snapshot!(input);
}

#[test]
fn string_literal() {
    let input = "\"hello world\"";
    assert_lex_snapshot!(input);
}

#[test]
fn string_literal_empty() {
    let input = "\"\"";
    assert_lex_snapshot!(input);
}

#[test]
fn string_literal_with_escaped_char() {
    let input = "\"hello\\nworld\\t\"";
    assert_lex_snapshot!(input);
}

#[test]
fn string_literal_with_escaped_quotes() {
    let input = "\"She said \\\"hello\\\"\"";
    assert_lex_snapshot!(input);
}

#[test]
fn string_literal_with_escaped_backslash() {
    let input = "\"backslash: \\\\\"";
    assert_lex_snapshot!(input);
}

#[test]
fn string_literal_with_special_chars() {
    let input = "\"hello!@#$%^&*()_+{}|:<>?~`-=[]\\;',./\"";
    assert_lex_snapshot!(input);
}

#[test]
fn symbols() {
    let input = "= == != >= <= : | || |> & && + - * / ^ % ! ? . .. , ; ( ) [ ] { } < > -> =>";
    assert_lex_snapshot!(input);
}

#[test]
fn slice_pattern_empty() {
    let input = "[]";
    assert_lex_snapshot!(input);
}

#[test]
fn slice_pattern_with_rest() {
    let input = "[first, ..rest]";
    assert_lex_snapshot!(input);
}

#[test]
fn slice_pattern_discard_rest() {
    let input = "[first, ..]";
    assert_lex_snapshot!(input);
}

#[test]
fn struct_keyword() {
    let input = "struct";
    assert_lex_snapshot!(input);
}

#[test]
fn struct_definition() {
    let input = r#"
struct Point {
  x: int,
  y: int,
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn struct_with_generics() {
    let input = r#"
struct Container<T> {
  value: T,
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn enum_keyword() {
    let input = "enum";
    assert_lex_snapshot!(input);
}

#[test]
fn enum_definition() {
    let input = r#"
enum Status {
  Pending,
  Complete,
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn enum_with_variants() {
    let input = r#"
enum Result<T, E> {
  Ok(T),
  Err(E),
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn impl_keyword() {
    let input = "impl";
    assert_lex_snapshot!(input);
}

#[test]
fn impl_block() {
    let input = r#"
impl Counter {
  fn new() -> Counter {
    return Counter { value: 0 };
  }
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn impl_with_generics() {
    let input = r#"
impl<T> Container<T> {
  fn get(self: Container<T>) -> T {
    return self.value;
  }
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn interface_keyword() {
    let input = "interface";
    assert_lex_snapshot!(input);
}

#[test]
fn interface_definition() {
    let input = r#"
interface Display {
  fn fmt() -> string;
}
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn type_keyword() {
    let input = "type";
    assert_lex_snapshot!(input);
}

#[test]
fn type_alias() {
    let input = r#"
type UserId = int;
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn char_escaped_carriage_return() {
    let input = "'\\r'";
    assert_lex_snapshot!(input);
}

#[test]
fn char_escaped_null() {
    let input = "'\\0'";
    assert_lex_snapshot!(input);
}

#[test]
fn compound_assignment() {
    let input = "x += 1; y -= 2; z *= 3; a /= 4; b %= 5;";
    assert_lex_snapshot!(input);
}

#[test]
fn range_exclusive() {
    let input = "0..10";
    assert_lex_snapshot!(input);
}

#[test]
fn range_inclusive() {
    let input = "0..=10";
    assert_lex_snapshot!(input);
}

#[test]
fn range_from() {
    let input = "0..";
    assert_lex_snapshot!(input);
}

#[test]
fn range_to() {
    let input = "..10";
    assert_lex_snapshot!(input);
}

#[test]
fn range_to_inclusive() {
    let input = "..=10";
    assert_lex_snapshot!(input);
}

#[test]
fn range_full() {
    let input = "..";
    assert_lex_snapshot!(input);
}

#[test]
fn range_with_float_ambiguity() {
    let input = "0..5";
    assert_lex_snapshot!(input);
}

#[test]
fn try_keyword() {
    let input = "try";
    assert_lex_snapshot!(input);
}

#[test]
fn try_block() {
    let input = r#"
let result = try {
  risky()?
};
"#;
    assert_lex_snapshot!(input);
}

#[test]
fn unicode_greek_letters() {
    let input = "θ α β λ π";
    assert_lex_snapshot!(input);
}

#[test]
fn unicode_mixed_with_ascii() {
    let input = "fn polar(r: float64, θ: float64) -> complex128";
    assert_lex_snapshot!(input);
}

#[test]
fn unicode_chinese() {
    let input = "let 北京 = \"Beijing\"";
    assert_lex_snapshot!(input);
}

#[test]
fn octal_basic() {
    let input = "0o755";
    assert_lex_snapshot!(input);
}

#[test]
fn octal_uppercase_prefix() {
    let input = "0O755";
    assert_lex_snapshot!(input);
}

#[test]
fn octal_with_underscores() {
    let input = "0o777_777";
    assert_lex_snapshot!(input);
}

#[test]
fn octal_legacy() {
    let input = "0755";
    assert_lex_snapshot!(input);
}

#[test]
fn octal_legacy_with_underscores() {
    let input = "0644_755";
    assert_lex_snapshot!(input);
}

#[test]
fn binary_basic() {
    let input = "0b1010";
    assert_lex_snapshot!(input);
}

#[test]
fn binary_uppercase_prefix() {
    let input = "0B1010";
    assert_lex_snapshot!(input);
}

#[test]
fn binary_with_underscores() {
    let input = "0b1111_0000";
    assert_lex_snapshot!(input);
}

#[test]
fn binary_long() {
    let input = "0b11111111_11111111";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_integer() {
    let input = "4i";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_float() {
    let input = "3.14i";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_scientific() {
    let input = "1e10i";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_with_underscores() {
    let input = "1_000i";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_zero() {
    let input = "0i";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_identifier_i() {
    let input = "i";
    assert_lex_snapshot!(input);
}

#[test]
fn imaginary_in_expression() {
    let input = "3 + 4i";
    assert_lex_snapshot!(input);
}

#[test]
fn nested_tuple_access() {
    let input = "nested.0.0";
    assert_lex_snapshot!(input);
}

#[test]
fn asi_after_range_from() {
    let input = "let r = 5..
let x = 42";
    assert_lex_snapshot!(input);
}

#[test]
fn asi_after_question_mark() {
    let input = "expr?
(a, b)";
    assert_lex_snapshot!(input);
}

#[test]
fn no_asi_before_closing_paren() {
    let input = "apply(
    10,
    |x| { x * 3 }
)";
    assert_lex_snapshot!(input);
}

#[test]
fn no_asi_before_closing_bracket() {
    let input = "arr[
    0
]";
    assert_lex_snapshot!(input);
}
