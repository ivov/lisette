use crate::_harness::build::compile_check;
use crate::_harness::filesystem::MockFileSystem;
use crate::_harness::formatting::{format_diagnostic_for_snapshot, format_diagnostic_standalone};
use crate::_harness::infer::infer_module;
use crate::{
    assert_desugar_error_snapshot, assert_infer_error_snapshot, assert_lex_error_snapshot,
    assert_multimodule_infer_error_snapshot, assert_parse_error_snapshot,
};

use diagnostics::module_graph::import_cycle;
use semantics::store::ENTRY_MODULE_ID;

#[test]
fn infer_nil_not_supported() {
    let input = r#"
fn test() -> Option<int> {
  nil
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_len() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  len(items)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_cap() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  cap(items)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_make() {
    let input = r#"
fn test() {
  let ch = make(10);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_append() {
    let input = r#"
fn test(items: Slice<int>) -> Slice<int> {
  append(items, 1)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_close() {
    let input = r#"
fn test(ch: Channel<int>) {
  close(ch);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_copy() {
    let input = r#"
fn test(dst: Slice<int>, src: Slice<int>) {
  copy(dst, src);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_delete() {
    let input = r#"
fn test(m: Map<string, int>) {
  delete(m, "key");
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_new() {
    let input = r#"
fn test() {
  let p = new(int);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_print() {
    let input = r#"
fn test(name: string) {
  print(name)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_builtin_println() {
    let input = r#"
fn test(name: string) {
  println(name)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_as_binding_in_let() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> int {
  let Point { x, .. } as q = p;
  q.x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_as_binding_in_for() {
    let input = r#"
struct Point { x: int, y: int }

fn test(pts: Slice<Point>) {
  for Point { x, .. } as p in pts {}
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_as_binding_in_param() {
    let input = r#"
struct Point { x: int, y: int }

fn test(Point { x, .. } as p: Point) -> int { x }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_nested_as_binding_in_let() {
    let input = r#"
struct Point { x: int, y: int }

fn test(pair: (Point, int)) -> int {
  let (Point { x, .. } as p, z) = pair;
  p.x + z
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_nested_as_binding_in_for() {
    let input = r#"
struct Point { x: int, y: int }

fn test(pairs: Slice<(Point, int)>) {
  for (Point { x, .. } as p, _) in pairs {}
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_nested_as_binding_in_param() {
    let input = r#"
struct Point { x: int, y: int }

fn test(pair: (Point, int), (Point { x, .. } as p, z): (Point, int)) -> int { x + z }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_mutate_match_arm_binding() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn bump(self: Ref<Counter>) {
    self.n = self.n + 1
  }
}

fn test(opt: Option<Counter>) {
  if let Some(Counter { n, .. } as c) = opt {
    c.bump();
    let _ = n;
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_underscore_as_alias() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> int {
  match p {
    Point { x, .. } as _ => x,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_uppercase_as_alias() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> int {
  match p {
    Point { x, .. } as P => x,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_redundant_as_identifier() {
    let input = r#"
fn test(x: int) -> int {
  match x {
    n as m => m,
    _ => 0,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_redundant_as_wildcard() {
    let input = r#"
fn test(x: int) -> int {
  match x {
    _ as m => m,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_redundant_as_literal() {
    let input = r#"
fn test(x: int) -> int {
  match x {
    42 as m => m,
    _ => 0,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_mut_not_allowed_with_destructuring() {
    let input = r#"
fn test() {
  let mut (a, b) = (1, 2);
  a
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn lex_too_many_slashes() {
    let input = "//// This has too many slashes";
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unterminated_string() {
    let input = r#"let x = "unterminated"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unterminated_char() {
    let input = r#"let x = 'a"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_empty_char() {
    let input = r#"let x = ''"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_invalid_escape() {
    let input = r#"let x = '\q'"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_octal_escape_out_of_range() {
    let input = r#"let x = "\400""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_missing_braces() {
    let input = r#"let x = "\u1F600""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_empty() {
    let input = r#"let x = "\u{}""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_invalid_hex() {
    let input = r#"let x = "\u{XYZ}""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_too_many_digits() {
    let input = r#"let x = "\u{1234567}""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_above_max() {
    let input = r#"let x = "\u{110000}""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_surrogate() {
    let input = r#"let x = "\u{D800}""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unicode_escape_unterminated() {
    let input = "let x = \"\\u{1F600\"";
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_number_trailing_underscore() {
    let input = r#"let x = 42_"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_number_consecutive_underscores() {
    let input = r#"let x = 1__000"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_float_decimal_leading_underscore() {
    let input = r#"let x = 3._14"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_hex_missing_digits() {
    let input = r#"let x = 0x"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_octal_missing_digits() {
    let input = r#"let x = 0o"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_octal_invalid_digit() {
    let input = r#"let x = 0o789"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_binary_missing_digits() {
    let input = r#"let x = 0b"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_binary_invalid_digit() {
    let input = r#"let x = 0b123"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_scientific_missing_exponent() {
    let input = r#"let x = 1e"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_scientific_missing_exponent_after_sign() {
    let input = r#"let x = 1e+"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_format_string_unterminated() {
    let input = r#"let x = f"hello {name}"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_format_string_unclosed_brace() {
    let input = r#"let x = f"hello {name""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_format_string_unmatched_brace() {
    let input = r#"let x = f"hello }name}""#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unexpected_character() {
    let input = r#"let x = 42 ~ invalid"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_unterminated_escape() {
    let input = "let x = '\\";
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_hex_imaginary() {
    let input = r#"let x = 0x10i"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_octal_imaginary() {
    let input = r#"let x = 0o10i"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_binary_imaginary() {
    let input = r#"let x = 0b10i"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn parse_pub_impl_block() {
    let input = r#"
struct Foo { x: int }

pub impl Foo {
  fn bar(self) -> int {
    self.x
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_misplaced_type_args_on_type_not_method() {
    let input = r#"
fn main() {
  let x = Slice<int>.new()
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_missing_closing_brace() {
    let input = r#"
fn main() {
  let x = 42;
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_expected_expression() {
    let input = r#"
fn main() {
  let x = ;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_invalid_token_in_pattern() {
    let input = r#"
fn test() {
  match x {
    + => 1,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_match_arm_missing_comma() {
    let input = r#"
fn test(x: int) -> int {
  match x {
    1 => 10
    2 => 20,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_match_arm_block_missing_comma() {
    let input = r#"
fn test(x: int) {
  match x {
    1 => {
      let _ = 10
    }
    2 => {
      let _ = 20
    }
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_struct_field_invalid_token() {
    let input = r#"
struct Foo {
  let x = 1
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_enum_variant_invalid_token() {
    let input = r#"
enum Foo {
  let x = 1
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_function_call_missing_comma() {
    let input = r#"
fn test() {
  foo(1 2 3);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_array_literal_missing_comma() {
    let input = r#"
fn test() {
  let arr = [1 2 3];
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_struct_instantiation_missing_comma() {
    let input = r#"
fn test() {
  let p = Point { x: 1 y: 2 };
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_lambda_return_type_requires_block() {
    let input = r#"
fn test() {
  let f = |x: int| -> int x * 2;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_impl_block_non_fn_token() {
    let input = r#"
struct Num { value: int }

impl Num {
  x
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_slice_pattern_unexpected_token() {
    let input = r#"
fn test() {
  match items {
    [+ + +] => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_float_pattern_not_allowed() {
    let input = r#"
fn test(x: float64) -> int {
  match x {
    3.14 => 1,
    _ => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_imaginary_pattern_not_allowed() {
    let input = r#"
fn test(x: complex128) -> int {
  match x {
    4i => 1,
    _ => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_inclusive_range_without_end() {
    let input = "fn test() { let r = 1..=; }";
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_inclusive_range_full_without_end() {
    let input = "fn test() { let r = ..=; }";
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_chained_range() {
    let input = "fn test() { let r = 0..1..2; }";
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_detached_doc_comment() {
    let input = r#"
/// Provides utilities for working with strings.
import "some_module"
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_detached_doc_comment_before_eof() {
    let input = r#"
fn foo() {}

/// Returns the current timestamp.
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_detached_doc_comment_before_impl() {
    let input = r#"
struct Foo {}

/// Methods for working with Foo.
impl Foo {
  fn bar(self: Foo) {}
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_single_element_tuple() {
    let input = r#"
fn test() {
  let t = (1,);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_single_element_tuple_type() {
    let input = r#"
fn f() -> (int,) {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_enum_variant_missing_paren() {
    let input = r#"
enum E { A(int }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_generic_missing_closing_angle() {
    let input = r#"
fn test() {
  let x: Foo<Bar = 1;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_unknown_directive() {
    let input = r#"
fn test() { @unknown(foo); }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_trailing_plus_in_bounds() {
    let input = r#"
fn f<T: Display +>() {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_local_enum_in_function() {
    let input = r#"
fn test() {
  enum Color { Red, Green, Blue }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_local_struct_in_function() {
    let input = r#"
fn test() {
  struct Data { name: string }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn desugar_propagate_in_pipeline() {
    let input = "fn test() { x |> validate? |> transform; }";
    assert_desugar_error_snapshot!(input);
}

#[test]
fn desugar_pipeline_with_literal() {
    let input = "fn test() { x |> 5; }";
    assert_desugar_error_snapshot!(input);
}

#[test]
fn desugar_pipeline_with_lambda() {
    let input = "fn test() { x |> |y| y * 2; }";
    assert_desugar_error_snapshot!(input);
}

#[test]
fn desugar_pipeline_with_binary() {
    let input = "fn test() { x |> 1 + 2; }";
    assert_desugar_error_snapshot!(input);
}

#[test]
fn infer_enum_variant_arity_mismatch() {
    let input = r#"
fn test() {
  let x = Some(42, 43);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_enum_variant_as_bare_value() {
    let input = r#"
enum A {
  Test { test: string },
}

fn main() {
  let a = A.Test
  let _ = a
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_enum_variant_as_bare_value_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub enum Shape {
  Rect { w: float64, h: float64 },
}
"#,
    );

    let source = r#"
import "shapes"

fn main() {
  let r = shapes.Shape.Rect
  let _ = r
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_enum_variant_not_found() {
    let input = r#"
fn test() {
  let x = Maybe(42);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_enum_variant_not_found_in_pattern() {
    let input = r#"
enum Status { Active, Inactive }

fn test() {
  let s: Status = Active;
  match s {
    Nope => {}
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_enum_variant_not_found_in_pattern_unqualified() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub enum Shape { Circle(float64), Rectangle(float64, float64) }
"#,
    );

    let source = r#"
import "shapes"

fn test() {
  let s = shapes.Shape.Circle(1.0);
  match s {
    Circle(r) => {}
  }
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_enum_variant_not_found_in_pattern_close_match() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub enum Shape { Circle(float64), Rectangle(float64, float64) }
"#,
    );

    let source = r#"
import "shapes"

fn test() {
  let s = shapes.Shape.Circle(1.0);
  match s {
    Circl(r) => {}
  }
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_function_arity_too_few() {
    let input = r#"
fn test() {
  let add = |x: int, y: int| -> int { x + y };
  add(5)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_function_arity_too_many() {
    let input = r#"
fn test() {
  let add = |x: int, y: int| -> int { x + y };
  add(5, 10, 15)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_function_type_mismatch() {
    let input = r#"
fn test() {
  let apply = |f: fn(int) -> int, x: int| -> int { f(x) };

  let two_param_fn = |a: int, b: int| a + b;
  apply(two_param_fn, 5)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_nested_function() {
    let input = r#"
fn main() {
  fn nested() -> int {
    42
  }
  nested()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_misplaced_type_alias_in_function() {
    let input = r#"
fn main() {
  type Score = int
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_misplaced_import_in_function() {
    let input = r#"
fn main() {
  import "go:strings"
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_misplaced_impl_in_function() {
    let input = r#"
struct Foo { x: int }

fn main() {
  impl Foo {
    fn bar(self) -> int { self.x }
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_misplaced_interface_in_function() {
    let input = r#"
fn main() {
  interface Greeter {
    fn greet(self) -> string
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_go_style_short_declaration() {
    let input = r#"
fn main() {
  x := 42
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_duplicate_function() {
    let input = r#"
fn greet() -> string { "hello" }
fn greet() -> string { "world" }

fn main() {
  greet()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_struct() {
    let input = r#"
struct Point { x: int, y: int }
struct Point { x: float64, y: float64 }

fn main() {
  let _ = Point { x: 1, y: 2 }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_enum() {
    let input = r#"
enum Dir { North, South }
enum Dir { East, West }

fn main() {
  let _ = Dir.North
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_const() {
    let input = r#"
const MAX = 100
const MAX = 200

fn main() {
  let _ = MAX
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_type_alias() {
    let input = r#"
type Id = int
type Id = string

fn main() {
  let _x: Id = 1
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_argument_count() {
    let input = r#"
fn test() {
  let x: Option<int, string> = Some(42);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_if_without_else_is_unit() {
    let input = r#"
fn test() -> int {
  if true { 42 }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_branch_type_mismatch() {
    let input = r#"
fn test() {
  let x = if true { 42 } else { "hello" };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_numeric() {
    let input = r#"
fn test() {
  let x = -true;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_orderable() {
    let input = r#"
fn test() {
  let f = |x: int| x + 1;
  let result = f > f;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_complex_not_orderable() {
    let input = r#"
fn test(a: complex64, b: complex64) -> bool {
  a < b
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_unindexable_type() {
    let input = r#"
fn test() {
  let x = 42;
  x[0]
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_callable() {
    let input = r#"
fn test() {
  let x = 42;
  x(1, 2)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_callable_suggests_as_cast_for_primitive_type_name() {
    let input = r#"
fn test(contents: Slice<byte>) {
  let _ = string(contents)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_conversion_arity() {
    let input = r#"
type Transformer = fn(int) -> int

fn test() {
  let f = |x: int| x * 2
  let _ = Transformer(f, f)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_member_not_found_with_suggestion() {
    let input = r#"
struct Point { x: int, y: int }

fn test() {
  let p = Point { x: 1, y: 2 };
  p.yy
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_method_not_found_with_typo_suggestion() {
    let input = r#"
fn test(s: string) -> bool {
  s.cntains("world")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_method_not_found_with_prefix_suggestion() {
    let input = r#"
fn test(s: string) -> int {
  s.len()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_method_not_found_with_prefix_suggestion_on_slice() {
    let input = r#"
fn test(s: Slice<int>) -> int {
  s.len()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_missing_fields() {
    let input = r#"
struct Person {
  name: string,
  age: int,
  email: string,
}

fn test() {
  let p = Person {
    name: "Alice",
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_zero_fill_no_zero_for_field() {
    let input = r#"
struct Bad {
  ok: int,
  bad: Channel<int>,
}

fn test() {
  let p = Bad { ok: 1, .. };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_zero_fill_no_zero_for_ref_field() {
    let input = r#"
struct Bad {
  ok: int,
  bad: Ref<int>,
}

fn test() {
  let p = Bad { ok: 1, .. };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_zero_fill_tuple_chain() {
    let input = r#"
struct Outer { t: (int, Channel<int>) }

fn test() {
  let _o = Outer { .. };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_zero_fill_struct_chain() {
    let input = r#"
struct Inner { bad: Channel<int> }
struct Outer { inner: Inner }

fn test() {
  let _o = Outer { .. };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_enum_struct_variant_zero_fill_no_zero_for_field() {
    let input = r#"
enum Action {
  Move { x: int, dst: Channel<int> },
  Stop,
}

fn test() {
  let m = Action.Move { x: 5, .. };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_struct_not_found() {
    let input = r#"
fn test() {
  let p = UnknownStruct { x: 1, y: 2 };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_propagate_in_function_returning_unit() {
    let input = r#"
fn test() {
  let x = Some(42)?;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_string_not_indexable() {
    let input = r#"
fn test(s: string) -> byte {
  s[0]
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_mismatch_int_string() {
    let input = r#"
fn test() {
  let x: int = "hello";
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_mismatch_return() {
    let input = r#"
fn get_number() -> int {
  let x = 1;
  let y = 2;
  let z = 3;
  return "not a number";
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_not_found() {
    let input = r#"
fn test() {
  let x: UnknownType = 42;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_not_found_struct_bound() {
    let input = r#"
struct Foo<T: Undefined> {}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_not_found_enum_bound() {
    let input = r#"
enum Foo<T: Undefined> {
  Bar(T)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_not_found_interface_bound() {
    let input = r#"
interface Foo<T: Undefined> {
  fn get(self) -> T
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_not_found_type_alias_bound() {
    let input = r#"
type Foo<T: Undefined> = T
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_variable_not_found_no_suggestion() {
    let input = r#"
fn test() {
  let x = unknown_variable;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_variable_not_found_with_suggestion() {
    let input = r#"
fn test() {
  let counter = 42;
  countr
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_variable_not_mutable() {
    let input = r#"
fn test() {
  let x = 10;
  x = 20;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_value_receiver_not_mutable() {
    let input = r#"
struct Counter { count: int }

impl Counter {
  fn increment(self: Counter) {
    self.count = self.count + 1;
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_receiver_not_named_self() {
    let input = r#"
struct Counter { count: int }

impl Counter {
  fn get_count(this: Counter) -> int {
    this.count
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_self_in_static_method() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn new(x: int, y: int) -> Point {
    Point { x: self.x, y: y }
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_receiver_type_mismatch() {
    let input = r#"
struct Counter { count: int }
struct Point { x: int, y: int }

impl Counter {
  fn wrong(self: Point) -> int {
    0
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_stringer_signature_mismatch_returns_int() {
    let input = r#"
struct A { a: string }

impl A {
  fn String(self) -> int {
    42
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_stringer_signature_mismatch_lowercase_extra_param() {
    let input = r#"
struct A { a: string }

impl A {
  fn string(self, prefix: string) -> string {
    prefix + self.a
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_go_stringer_signature_mismatch() {
    let input = r#"
struct A { a: string }

impl A {
  fn GoString(self) -> int {
    0
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_unknown_outside_typedef() {
    let input = r#"
fn test() {
  let x: Unknown = 42;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_unknown_type_mismatch() {
    let input = r#"
fn process(x: int) -> int { x }
fn test() {
  let data = get_unknown();
  process(data)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_match_on_unconstrained_type() {
    let input = r#"
fn get_something<T>() -> T {
  return get_something();
}

fn main() {
  let x = get_something();
  match x {
    (a, b) => a,
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_option_where_inner_expected() {
    let input = r#"
fn process(x: int) -> int { x }
fn test() {
  let opt = Option.Some(42);
  process(opt)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_slice_where_element_expected() {
    let input = r#"
fn process(x: int) -> int { x }
fn test() {
  let items = [1, 2, 3];
  process(items)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_element_where_slice_expected() {
    let input = r#"
fn process(x: Slice<int>) -> Slice<int> { x }
fn test() {
  let item = 42;
  process(item)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_inner_where_option_expected() {
    let input = r#"
fn test() -> Option<int> {
  return 42;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_inner_where_result_expected() {
    let input = r#"
fn test() -> Result<int, string> {
  return 42;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn match_redundant_pattern() {
    let input = r#"
enum Status { Active, Inactive }

fn test() {
  let s: Status = Active;
  match s {
    Active => 1,
    Inactive => 2,
    Active => 3,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn match_redundant_after_wildcard() {
    let input = r#"
enum Status { Active, Inactive }

fn test() {
  let s: Status = Active;
  match s {
    _ => 0,
    Active => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn match_redundant_or_pattern_duplicate() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test() {
  let c: Color = Red;
  match c {
    Red | Red => 1,
    Green | Blue => 2,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_tuple_size_mismatch() {
    let input = r#"
fn test() {
  let pair = (1, 2);
  let (a, b, c) = pair;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_tuple_pattern_arity_mismatch() {
    let input = r#"
fn test() {
  let (a, b, c): (int, int) = (1, 2);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_tuple_too_large_expression() {
    let input = r#"
fn test() {
  let x = (1, 2, 3, 4, 5, 6);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_tuple_too_large_type() {
    let input = r#"
fn test(x: (int, int, int, int, int, int)) {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_tuple_too_large_pattern() {
    let input = r#"
fn test() {
  let (a, b, c, d, e, f) = get_tuple();
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn match_non_exhaustive() {
    let input = r#"
fn test() {
  let r: Result<int, string> = Ok(42);
  match r {
    Ok(x) => x,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_uninferred_binding_type() {
    let input = r#"
fn test() {
  let x = [];
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cannot_infer_type_arguments() {
    let input = r#"
fn test() {
  let ch = Channel.new();
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_addition_type_mismatch() {
    let input = r#"
fn test() {
  let x = "hello" + 5;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_subtraction_type_mismatch() {
    let input = r#"
fn test() {
  let x = "hello" - 5;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_comparison_type_mismatch() {
    let input = r#"
fn test() {
  let x = 42 < "hello";
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_equality_type_mismatch() {
    let input = r#"
fn test() {
  let x = 42 == "hello";
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_duplicate_struct_field_in_instantiation() {
    let input = r#"
struct Point { x: int, y: int }

fn test() {
  Point { x: 1, y: 2, x: 3 }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_slice_index_type_mismatch() {
    let input = r#"
fn test() {
  let items = [1, 2, 3];
  items["key"]
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_range_to_not_iterable() {
    let input = r#"
import "go:fmt"

fn test() {
  for i in ..10 {
    fmt.Print(f"{i}");
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_range_to_inclusive_not_iterable() {
    let input = r#"
import "go:fmt"

fn test() {
  for i in ..=10 {
    fmt.Print(f"{i}");
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_non_int_range_not_iterable() {
    let input = r#"
import "go:fmt"

fn test() {
  for c in 'a'..'z' {
    fmt.Print(f"{c}");
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_range_index_on_non_slice() {
    let input = r#"
fn test(m: Map<string, int>) {
  m[0..3]
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_interface_bound_not_satisfied() {
    let input = r#"
interface Writer {
  fn write(data: string) -> int;
}

fn use_writer(w: Writer) -> int {
  return w.write("hello");
}

fn test() {
  use_writer(42)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_interface_bound_wrong_arity() {
    let input = r#"
interface Writer {
  fn write(data: string) -> int;
}

struct File { path: string }

impl File {
  fn write(self: File) -> int {
    return 0;
  }
}

fn use_writer(w: Writer) -> int {
  return w.write("hello");
}

fn test() {
  use_writer(File { path: "test.txt" })
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_interface_bound_multiple_issues() {
    let input = r#"
interface ReadWriter {
  fn read() -> string;
  fn write(data: string) -> int;
}

struct File { path: string }

impl File {
  fn write(self: File, data: string) -> string {
    return "ok";
  }
}

fn use_rw(rw: ReadWriter) -> int {
  return rw.write("hello");
}

fn test() {
  use_rw(File { path: "test.txt" })
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_interface_bound_incompatible_signature() {
    let input = r#"
interface Writer {
  fn write(data: string) -> int;
}

struct File { path: string }

impl File {
  fn write(self: File, data: string) -> string {
    return "ok";
  }
}

fn use_writer(w: Writer) -> int {
  return w.write("hello");
}

fn test() {
  use_writer(File { path: "test.txt" })
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_pointer_receiver_interface_mismatch() {
    let input = r#"
interface Worker {
  fn name(self) -> string
  fn work(self) -> int
}

struct MyWorker { label: string, count: int }

impl MyWorker {
  fn name(self) -> string { self.label }
  fn work(self: Ref<MyWorker>) -> int { self.count }
}

fn use_worker(w: Worker) -> string { w.name() }

fn test() {
  let w = MyWorker { label: "test", count: 0 }
  use_worker(w)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn builtin_generic_type_satisfies_interface_bound() {
    let input = r#"
interface HasLength {
  fn length() -> int;
}

fn print_length<T: HasLength>(item: T) -> int {
  item.length()
}

fn main() -> int {
  print_length([1, 2, 3])
}
"#;
    let result = crate::_harness::infer::infer(input);
    result.assert_no_errors();
}

#[test]
fn infer_interface_inheritance_both_missing() {
    let input = r#"
interface Display {
  fn show() -> string;
}

interface Logger {
  impl Display;
  fn log() -> ();
}

struct File { path: string }

fn use_logger(l: Logger) {
  l.log();
}

fn test() {
  use_logger(File { path: "test.txt" })
}
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(!result.errors.is_empty(), "Expected errors");

    let mut output = String::new();
    for (i, error) in result.errors.iter().enumerate() {
        if i > 0 {
            output.push_str("\n---\n\n");
        }
        output.push_str(&format_diagnostic_for_snapshot(error, input, "test.lis"));
    }

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn module_graph_import_cycle() {
    let cycle = vec![
        "module_a".to_string(),
        "module_b".to_string(),
        "module_c".to_string(),
        "module_a".to_string(),
    ];

    let diagnostic = import_cycle(&cycle);
    let output = format_diagnostic_standalone(&diagnostic);

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn module_graph_import_self_loop() {
    let cycle = vec!["module_a".to_string(), "module_a".to_string()];

    let diagnostic = import_cycle(&cycle);
    let output = format_diagnostic_standalone(&diagnostic);

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn parse_slice_pattern_suffix() {
    let input = r#"
fn test(items: Slice<int>) {
  match items {
    [..rest, last] => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_nested_call() {
    let input = r#"
fn test() {
  foo(bar(baz()^));
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_multi_arg() {
    let input = r#"
fn test() {
  foo(a, b^c, d);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_expr_inside() {
    let input = r#"
fn test() {
  foo(1 + ^);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_slice_pattern() {
    let input = r#"
fn test() {
  let [a^ b, c] = arr;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_tuple_pattern() {
    let input = r#"
fn test() {
  let (a^ b, c) = t;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_struct_pattern() {
    let input = r#"
fn test() {
  let Point { x^ y } = p;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_interface() {
    let input = r#"
interface Foo {
  ^
  fn bar();
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_struct_def() {
    let input = r#"
struct Point {
  x: i32,
  ^
  y: i32,
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_enum_def() {
    let input = r#"
enum Status {
  Ok,
  ^
  Error,
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_match() {
    let input = r#"
fn test(x: i32) -> i32 {
  match x {
    1 => 10,
    ^
    2 => 20,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_import_invalid_path() {
    let input = r#"
import foo.bar;
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_unclosed_type_args() {
    let input = r#"
fn test() {
  let x: Option<Result<Either<Box<Ref<Map<int,
  let y = 5;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_trailing_comma_in_type_args() {
    let input = r#"
fn test() {
  let x: Result<int, > = 5;
  let y = 10;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_unclosed_type_args_wrong_bracket() {
    let input = r#"
fn test(x: Slice<int) {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_lambda_missing_type() {
    let input = r#"
fn test() {
  let f = |x: | x + 1;
  let g = 5;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_nested_unclosed_parens() {
    let input = r#"
fn test() {
  let x = ((((1 + 2;
  let y = 5;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_error_recovery_unclosed_bracket() {
    let input = r#"
fn test() {
  let arr = [1, 2, 3;
  let y = 5;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn lex_format_string_unclosed_brace_at_newline() {
    let input = "let s = f\"hello {name\nlet x = 1";
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_format_string_multiline_interpolation() {
    let input = "let s = f\"result: {\n  match n {\n    0 => \"zero\",\n  }\n}\"";
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_format_string_escaped_quotes_in_interpolation() {
    let input = "let s = f\"x: {func(\\\"arg\\\")}\"";
    assert_lex_error_snapshot!(input);
}

#[test]
fn infer_opaque_type_outside_typedef() {
    let input = r#"
type Point
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_bodyless_function_outside_typedef() {
    let input = r#"
fn greet()
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_valueless_const_outside_typedef() {
    let input = r#"
const MAX_SIZE: int
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_valueless_const_missing_annotation_in_typedef() {
    let mut fs = MockFileSystem::new();
    fs.add_file("types", "consts.d.lis", "const MAX_SIZE");
    infer_module("types", fs).assert_infer_code("valueless_const_missing_annotation");
}

#[test]
fn module_graph_module_not_found() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "nonexistent""#);
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected module not found error");

    let output =
        format_diagnostic_for_snapshot(&result.errors[0], r#"import "nonexistent""#, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn module_graph_go_stdlib_hint() {
    let source = r#"import "time""#;
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", source);
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected module not found error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn module_graph_src_prefix_hint() {
    let source = r#"import "src/math""#;
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", source);
    fs.add_file(
        "math",
        "math.lis",
        "pub fn add(a: int, b: int) -> int { a + b }",
    );
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected module not found error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn infer_cannot_import_prelude() {
    let source = r#"import "prelude""#;
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", source);
    let result = infer_module("main", fs);

    assert!(
        !result.errors.is_empty(),
        "Expected errors but inference succeeded"
    );

    let output = format_diagnostic_for_snapshot(&result.errors[0], source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn module_graph_nested_import_error_attribution() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "_entry_",
        "main.lis",
        r#"import "go:fmt"
import "outer"

fn main() {
  fmt.Println(outer.outer_fn())
}"#,
    );
    fs.add_file(
        "outer",
        "mod.lis",
        r#"import "inner"

pub fn outer_fn() -> string {
  f"inner: {inner.inner_fn()}"
}"#,
    );
    fs.add_file(
        "outer/inner",
        "mod.lis",
        r#"pub fn inner_fn() -> string {
  "hello"
}"#,
    );

    let result = infer_module("_entry_", fs);

    assert_eq!(
        result.errors.len(),
        1,
        "Expected exactly 1 error for bad import"
    );

    let output = format_diagnostic_for_snapshot(&result.errors[0], r#"import "inner""#, "mod.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn module_graph_test_file_rejected() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "_entry_",
        "main.lis",
        r#"import "math"

fn main() {
  let _ = math.add(1, 2)
}"#,
    );
    fs.add_file(
        "math",
        "core.lis",
        "pub fn add(a: int, b: int) -> int { a + b }",
    );
    fs.add_file(
        "math",
        "helpers_test.lis",
        "pub fn sub(a: int, b: int) -> int { a - b }",
    );

    let result = infer_module("_entry_", fs);

    assert_eq!(result.errors.len(), 1);
    assert!(
        result.errors[0]
            .code_str()
            .is_some_and(|c| c.contains("test_file_not_supported"))
    );
}

#[test]
fn infer_pattern_missing_field() {
    let input = r#"
struct Point { x: int, y: int }

fn main() {
  let p = Point { x: 1, y: 2 };
  let Point { x } = p;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_pattern_missing_fields_multiple() {
    let input = r#"
struct Point { x: int, y: int, z: int }

fn main() {
  let p = Point { x: 1, y: 2, z: 3 };
  let Point { x } = p;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_explicit_unit_return_type_mismatch() {
    let input = r#"
fn foo() -> () {
  123
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_empty_body_non_unit_return() {
    let input = r#"
fn foo() -> int {
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_duplicate_field_in_struct_pattern() {
    let input = r#"
struct Point { x: int, y: int }

fn main() {
  let p = Point { x: 1, y: 2 };
  let Point { x, x: x2, .. } = p;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_duplicate_binding_in_tuple_pattern() {
    let input = r#"
fn main() {
  let (x, x) = (1, 2);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_binding_in_slice_pattern() {
    let input = r#"
fn main() {
  let [x, x] = [1, 2];
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_binding_in_enum_pattern() {
    let input = r#"
enum Pair { A(int, int) }

fn main() {
  let Pair.A(x, x) = Pair.A(1, 2);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_binding_in_nested_pattern() {
    let input = r#"
fn main() {
  let (x, (y, x)) = (1, (2, 3));
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_binding_in_slice_rest_pattern() {
    let input = r#"
fn main() {
  let [x, ..x] = [1, 2, 3];
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_bounded_function_assigned_to_concrete_function_type() {
    let input = r#"
interface Display {
  fn show() -> string;
}

fn bounded<T: Display>(x: T) -> int {
  42
}

fn accept(f: fn(int) -> int) -> int {
  f(123)
}

fn main() {
  accept(bounded)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_refutable_slice_pattern_in_let() {
    let input = r#"
fn test(slice: Slice<int>) {
  let [a, b] = slice;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_refutable_enum_pattern_in_let() {
    let input = r#"
fn test(opt: Option<int>) {
  let Some(x) = opt;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_literal_pattern_in_let() {
    let input = r#"
fn test(x: int) {
  let 42 = x;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_tuple_does_not_implement_interface() {
    let input = r#"
interface Display {
  fn show() -> string;
}

fn print_value<T: Display>(value: T) -> string {
  return value.show();
}

fn main() {
  let tuple = (1, 2);
  print_value(tuple);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_function_type_does_not_implement_interface() {
    let input = r#"
interface Display {
  fn show() -> string;
}

fn print_value<T: Display>(value: T) -> string {
  return value.show();
}

fn some_func(x: int) -> int {
  return x;
}

fn main() {
  print_value(some_func);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_never_in_generic_expected_position() {
    let input = r#"
enum MyResult<T, E> {
  MyOk(T),
  MyErr(E),
}

fn main() {
  let x: MyResult<Never, int> = MyOk(1);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_variable_hint_preserves_param_names() {
    let input = r#"
enum Either<L, R> {
  Left(L),
  Right(R),
}

fn main() {
  let x: Either<int, string> = Left("oops");
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_let_else_must_diverge() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let Some(x) = opt else { 42 };
  x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_let_else_must_diverge_no_return() {
    let input = r#"
fn println(s: string) { }

fn test(opt: Option<int>) -> int {
  let Some(x) = opt else { println("oops"); };
  x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_param_with_args() {
    let input = r#"
fn test<T>(x: T<int>) -> T {
  x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_irrefutable_while_let() {
    let input = r#"
fn test() {
  let mut x = 0;
  while let y = x {
    x = x + 1;
    if x > 10 { break; }
    let _ = y;
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_irrefutable_while_let_or_pattern() {
    let input = r#"
fn test(opt: Option<int>) {
  while let Some(_) | None = opt {
    break;
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_args_on_non_generic() {
    let input = r#"
fn foo(x: int) -> int { x }

fn main() {
  let _ = foo<string>(42);
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_private_field_access() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Point {
  x: int,
  pub y: int,
}
"#,
    );

    let source = r#"
import "shapes"

fn main() -> int {
  let p = shapes.Point { x: 1, y: 2 };
  p.x
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_private_field_in_struct_literal() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Point {
  x: int,
  pub y: int,
}
"#,
    );

    let source = r#"
import "shapes"

fn main() {
  let p = shapes.Point { x: 1, y: 2 };
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_private_field_in_struct_literal_aliased_import() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Point {
  x: int,
  pub y: int,
}
"#,
    );

    let source = r#"
import s "shapes"

fn main() {
  let p = s.Point { x: 1, y: 2 };
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_private_field_in_pattern() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Point {
  x: int,
  pub y: int,
}

pub fn make_point() -> Point {
  Point { x: 1, y: 2 }
}
"#,
    );

    let source = r#"
import "shapes"

fn main() -> int {
  let p = shapes.make_point();
  match p {
    shapes.Point { x, y } => x + y,
  }
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_private_field_in_struct_spread() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Point {
  x: int,
  pub y: int,
}

pub fn make_point() -> Point {
  Point { x: 1, y: 2 }
}
"#,
    );

    let source = r#"
import "shapes"

fn main() {
  let p = shapes.make_point();
  let q = shapes.Point { y: 10, ..p };
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_private_field_in_struct_zero_fill_direct() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Point {
  x: int,
  pub y: int,
}
"#,
    );

    let source = r#"
import "shapes"

fn main() {
  let q = shapes.Point { y: 10, .. };
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_private_field_in_struct_zero_fill_transitive() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "other",
        "lib.lis",
        r#"
pub struct Inner {
  pub a: int,
  b: int,
}
"#,
    );

    let source = r#"
import "other"

struct Outer {
  inner: other.Inner,
}

fn main() {
  let o = Outer { .. };
  let _ = o;
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_or_pattern_binding_mismatch() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) | None => x,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_or_pattern_binding_mismatch_reversed() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    None | Some(x) => x,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_or_pattern_type_mismatch() {
    let input = r#"
fn test(res: Result<int, string>) -> int {
  match res {
    Ok(x) | Err(x) => x,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_or_pattern_in_let_binding() {
    let input = r#"
fn test(x: int) {
  let 1 | 2 = x;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_nested_or_pattern() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(1 | 2) => 1,
    _ => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_nested_or_pattern_in_parens() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some((1 | 2)) => 1,
    _ => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_or_pattern_in_select() {
    let input = r#"
fn test(ch: Receiver<int>) {
  select {
    let x | y = <-ch => (),
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_duplicate_impl_parent() {
    let input = r#"
interface A {}
interface I {
  impl A;
  impl A;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_try_block_unclosed() {
    let input = r#"fn f() { try { 1"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_enum_variant_missing_comma() {
    let input = r#"
enum E { V(int string) }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_empty_generic_bounds() {
    let input = r#"
fn f<T:>() {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_generic_bounds_missing_plus() {
    let input = r#"
interface Display {}
interface Clone {}
fn f<T: Display Clone>() {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_try_without_block() {
    let input = r#"
fn foo() -> int { 1 }
fn f() {
  try foo()
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_task_without_call() {
    let input = r#"
fn work() {}
fn f() {
  task work
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_task_dot_access_without_call() {
    let input = r#"
fn f() {
  task module.work
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_task_in_let_binding() {
    let input = r#"
fn work() {}
fn f() {
  let x = task work();
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_task_as_function_argument() {
    let input = r#"
fn work() {}
fn consume(x: ()) {}
fn f() {
  consume(task work());
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_task_in_slice_literal() {
    let input = r#"
fn a() {}
fn b() {}
fn f() {
  let arr = [task a(), task b()];
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_defer_without_call() {
    let input = r#"
fn cleanup() {}
fn f() {
  defer cleanup
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_defer_dot_access_without_call() {
    let input = r#"
fn f() {
  defer module.cleanup
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_defer_in_let_binding() {
    let input = r#"
fn cleanup() {}
fn f() {
  let x = defer cleanup();
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_as_function_argument() {
    let input = r#"
fn cleanup() {}
fn consume(x: ()) {}
fn f() {
  consume(defer cleanup());
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_in_slice_literal() {
    let input = r#"
fn a() {}
fn b() {}
fn f() {
  let arr = [defer a(), defer b()];
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_in_for_loop() {
    let input = r#"
fn cleanup() {}
fn f() {
  for i in 0..10 {
    defer cleanup();
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_in_while_loop() {
    let input = r#"
fn cleanup() {}
fn f() {
  let mut i = 0;
  while i < 10 {
    defer cleanup();
    i = i + 1;
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_in_loop() {
    let input = r#"
fn cleanup() {}
fn f() {
  loop {
    defer cleanup();
    break;
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_block_with_propagate() {
    let input = r#"
fn risky() -> Result<(), string> {
  Ok(())
}
fn f() -> Result<(), string> {
  defer {
    risky()?;
  };
  Ok(())
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_return_in_defer_block() {
    let input = r#"
fn f() -> int {
  defer {
    return 42;
  };
  0
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_break_in_defer_block() {
    let input = r#"
fn f() {
  defer {
    break;
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_continue_in_defer_block() {
    let input = r#"
fn f() {
  defer {
    continue;
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_in_block_as_let_binding() {
    let input = r#"
fn cleanup() {}
fn f() {
  let x = { defer cleanup(); };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_defer_in_block_as_function_argument() {
    let input = r#"
fn cleanup() {}
fn consume(x: ()) {}
fn f() {
  consume({ defer cleanup() });
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_task_in_block_as_let_binding() {
    let input = r#"
fn work() {}
fn f() {
  let x = { task work(); };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_task_in_block_as_function_argument() {
    let input = r#"
fn work() {}
fn consume(x: ()) {}
fn f() {
  consume({ task work() });
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_struct_pattern_rest_not_last() {
    let input = r#"
struct Point { x: int, y: int }
fn f(p: Point) -> int {
  match p { Point { ..rest, x } => 1 }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_blank_import_non_go() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn helper() -> int {
  42
}
"#,
    );

    let source = r#"
import _ "utils"

fn main() -> int {
  0
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_import_alias_collision() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "foo/utils",
        "lib.lis",
        r#"
pub fn helper() -> int { 1 }
"#,
    );

    fs.add_file(
        "bar/utils",
        "lib.lis",
        r#"
pub fn helper() -> int { 2 }
"#,
    );

    let source = r#"
import "foo/utils"
import "bar/utils"

fn main() {
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_duplicate_import() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn helper() -> int { 42 }
"#,
    );

    let source = r#"
import a "utils"
import b "utils"

fn main() {
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_duplicate_import_blank_after_named() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn helper() -> int { 42 }
"#,
    );

    let source = r#"
import u "utils"
import _ "utils"

fn main() {
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_try_block_empty() {
    let input = r#"
fn test() {
  let result = try {};
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_try_block_no_question_mark() {
    let input = r#"
fn test() {
  let result = try {
    let x = 42;
    x
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_try_block_return_outside_function() {
    let input = r#"
fn test() -> int {
  let result = try {
    if true {
      return 0;
    }
    Some(42)?
  };
  match result {
    Some(x) => x,
    None => 0,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_try_block_break_outside_loop() {
    let input = r#"
fn test() {
  let result = try {
    if true {
      break;
    }
    Some(42)?
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_try_block_continue_outside_loop() {
    let input = r#"
fn test() {
  let result = try {
    if true {
      continue;
    }
    Some(42)?
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_try_block_mixed_carriers() {
    let input = r#"
fn get_result() -> Result<int, string> { Ok(1) }
fn get_option() -> Option<int> { Some(2) }

fn test() {
  let result = try {
    let a = get_result()?;
    let b = get_option()?;
    a + b
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_try_block_annotation_mismatch() {
    let input = r#"
fn risky() -> Result<int, string> { Ok(42) }

fn test() {
  let result: Result<string, string> = try {
    risky()?
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_duplicate_struct_field_in_definition() {
    let input = r#"
struct S { x: int, x: string }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_duplicate_enum_struct_variant_field() {
    let input = r#"
enum E { Foo { x: int, x: int } }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_duplicate_enum_variant() {
    let input = r#"
enum E { A, A }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_duplicate_interface_method() {
    let input = r#"
interface I {
  fn f();
  fn f();
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_division_by_zero() {
    let input = r#"
fn main() {
  let x = 10 / 0;
  x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_remainder_by_zero() {
    let input = r#"
fn main() {
  let x = 10 % 0;
  x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_uppercase_binding_in_function_param() {
    let input = r#"
fn scale(X: int, Y: int) -> int {
  X + Y
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_keyword_as_binding() {
    let input = r#"
fn walk_dir(root: string, fn: string) {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_keyword_as_binding_in_for_loop() {
    let input = r#"
fn main() {
  let items = [1, 2, 3];
  for type in items {
    items
  }
}
"#;

    let lex_result = syntax::lex::Lexer::new(input, 0).lex();
    let parse_result = syntax::parse::Parser::new(lex_result.tokens, input).parse();

    assert!(
        parse_result.errors.len() == 1,
        "Expected exactly 1 error for keyword-as-binding in for loop, got {}: {:?}",
        parse_result.errors.len(),
        parse_result
            .errors
            .iter()
            .map(|e| &e.message)
            .collect::<Vec<_>>()
    );

    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_keyword_as_binding_in_match() {
    let input = r#"
fn main() {
  let x = Some(1);
  match x {
    Some(type) => 0,
    None => 0,
  }
}
"#;

    let lex_result = syntax::lex::Lexer::new(input, 0).lex();
    let parse_result = syntax::parse::Parser::new(lex_result.tokens, input).parse();

    assert!(
        parse_result.errors.len() == 1,
        "Expected exactly 1 error for keyword-as-binding in match, got {}: {:?}",
        parse_result.errors.len(),
        parse_result
            .errors
            .iter()
            .map(|e| &e.message)
            .collect::<Vec<_>>()
    );

    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_type_param_not_declared() {
    let input = r#"
struct Container<T> {
  value: T
}

impl Container<T> {
  fn get(self) -> T {}
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_value_enum_outside_typedef() {
    let input = r#"
enum Weekday {
  Sunday = 0,
  Monday = 1,
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_trait_instead_of_interface() {
    let input = r#"
trait Displayable {
  fn display() -> string
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_expected_declaration() {
    let input = r#"
123
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_regular_enum_with_underlying_type() {
    let input = r#"
enum Status: int {
  Active,
  Inactive,
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_negative_pattern_below_i64_min() {
    let input = r#"
fn classify(x: int) -> string {
  match x {
    -9223372036854775809 => "low",
    _ => "other",
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_value_enum_negative_below_i64_min() {
    let input = r#"
enum Bad: int64 {
  Way = -9223372036854775809,
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_value_enum_positive_above_u64_max() {
    let input = r#"
enum Bad: int64 {
  Way = 99999999999999999999999,
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_value_enum_negative_below_u64_max() {
    let input = r#"
enum Bad: int64 {
  Way = -18446744073709551616,
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_value_enum_with_generics() {
    let source = r#"
enum Status<T> {
  Active = 0,
  Inactive = 1,
}
"#;
    let mut fs = MockFileSystem::new();
    fs.add_file("types", "status.d.lis", source);
    let result = infer_module("types", fs);

    assert!(!result.errors.is_empty(), "Expected parse error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], source, "status.d.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn infer_invalid_division_by_numeric_alias() {
    let typedef_source = r#"
pub enum Duration: int64 {
  Second = 1000000000,
}
"#;
    let main_source = r#"
import "time"

fn test() {
  let x = 100 / time.Duration.Second;
}
"#;
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", typedef_source);
    fs.add_file("main", "main.lis", main_source);
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected type error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], main_source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn infer_invalid_remainder_by_numeric_alias() {
    let typedef_source = r#"
pub enum Duration: int64 {
  Second = 1000000000,
}
"#;
    let main_source = r#"
import "time"

fn test() {
  let x = 100 % time.Duration.Second;
}
"#;
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", typedef_source);
    fs.add_file("main", "main.lis", main_source);
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected type error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], main_source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn infer_cross_family_numeric_alias() {
    let typedef_source = r#"
pub enum Duration: int64 {
  Second = 1000000000,
}
"#;
    let main_source = r#"
import "time"

fn test() {
  let x = time.Duration.Second * 1.5;
}
"#;
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", typedef_source);
    fs.add_file("main", "main.lis", main_source);
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected type error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], main_source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn infer_different_numeric_aliases() {
    let typedef_source = r#"
pub enum DurationA: int64 {
  Second = 1000000000,
}

pub enum DurationB: int64 {
  Second = 1000000000,
}
"#;
    let main_source = r#"
import "time"

fn test() {
  let x = time.DurationA.Second + time.DurationB.Second;
}
"#;
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", typedef_source);
    fs.add_file("main", "main.lis", main_source);
    let result = infer_module("main", fs);

    assert!(!result.errors.is_empty(), "Expected type error");

    let output = format_diagnostic_for_snapshot(&result.errors[0], main_source, "main.lis");

    insta::with_settings!({
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(output);
    });
}

#[test]
fn infer_taking_value_of_ufcs_method() {
    let input = r#"
fn test() {
  let opt: Option<int> = Some(42);
  let f = opt.map;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_impl_item() {
    let input = r#"
struct Foo {}

impl Foo {
  fn bar(self: Foo) {}
}

impl Foo {
  fn bar(self: Foo) {}
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_interface_method_with_type_parameters() {
    let input = r#"
interface Mapper {
  fn map<U>(self, f: fn(int) -> U) -> U;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_use_instead_of_import() {
    let input = r#"
use fmt
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_go_channel_receive() {
    let input = r#"
fn test(ch: Receiver<int>) {
  let x = <-ch;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_go_channel_send() {
    let input = r#"
fn test(ch: Channel<int>) {
  ch <- 42;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_fn_as_lambda() {
    let input = r#"
fn main() {
  let doubled = [1, 2, 3].map(fn(x: int) -> int { x * 2 });
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_go_slice_syntax_in_type() {
    let input = r#"
fn test(arr: []int) {}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_double_colon_in_pattern() {
    let input = r#"
pub enum Shape { Circle(int), Rectangle { width: int, height: int } }

fn test(s: Shape) -> int {
  match s {
    Shape::Circle(r) => r,
    _ => 0,
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_double_colon_in_expression() {
    let input = r#"
pub enum Shape { Circle(int) }

fn test() {
  let s = Shape::Circle(5);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_turbofish() {
    let input = r#"
fn identity<T>(x: T) -> T { x }

fn test() {
  let x = identity::<int>(5);
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_ref_self() {
    let input = r#"
pub struct Counter { count: int }

impl Counter {
  fn get_count(&self) -> int {
    self.count
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_mut_ref_self() {
    let input = r#"
pub struct Counter { count: int }

impl Counter {
  fn set_count(&mut self, n: int) {
    self.count = n;
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_mut_ref() {
    let input = r#"
fn test() {
  let mut x = 5;
  let y = &mut x;
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_rust_impl_trait_for_type() {
    let input = r#"
interface Showable {
  fn show(self) -> string
}

struct Item { name: string }

impl Showable for Item {
  fn show(self) -> string {
    self.name
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_go_style_goroutine_block() {
    let input = r#"
fn test() {
  go {
    let x = 1
  }
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_go_style_goroutine_call() {
    let input = r#"
fn some_job() {}

fn test() {
  go some_job()
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_cannot_take_address_of_literal() {
    let input = r#"
fn takes_ref(n: Ref<int>) {
}

fn main() {
  takes_ref(&42)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cannot_take_address_of_binary_expression() {
    let input = r#"
fn takes_ref(n: Ref<int>) {
}

fn main() {
  takes_ref(&(1 + 2))
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_can_take_address_of_variable() {
    let input = r#"
fn takes_ref(n: Ref<int>) {
}

fn main() {
  let x = 42;
  takes_ref(&x)
}
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors but got: {:?}",
        result.errors
    );
}

#[test]
fn infer_can_take_address_of_struct_literal() {
    let input = r#"
struct Foo { value: int }

fn takes_ref(f: Ref<Foo>) {
}

fn main() {
  takes_ref(&Foo { value: 42 })
}
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors but got: {:?}",
        result.errors
    );
}

#[test]
fn infer_cannot_auto_address_map_index_receiver() {
    let input = r#"
struct Foo { value: int }

impl Foo {
  fn increment(self: Ref<Foo>) {
    self.value = self.value + 1
  }
}

fn main() {
  let m = Map.new<string, Foo>();
  m["key"].increment()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_recover_cannot_use_question_mark() {
    let input = r#"
fn fallible() -> Result<int, string> { Ok(42) }

fn test() {
  recover { fallible()? }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_recover_block_return() {
    let input = r#"
fn test() -> int {
  recover {
    return 42;
  };
  0
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_recover_block_break() {
    let input = r#"
fn test() {
  for i in [1, 2, 3] {
    recover { break };
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_recover_block_continue() {
    let input = r#"
fn test() {
  for i in [1, 2, 3] {
    recover { continue };
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_recover_block_empty() {
    let input = r#"
fn test() {
  recover {}
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_invalid_cast_string_to_int() {
    let input = r#"
fn test() -> int {
  "42" as int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_invalid_cast_bool_to_int() {
    let input = r#"
fn test() -> int {
  true as int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_invalid_cast_struct_to_int() {
    let input = r#"
struct MyStruct { x: int }

fn test() -> int {
  let s = MyStruct { x: 1 };
  s as int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_invalid_cast_complex_to_int() {
    let input = r#"
fn test() -> int {
  let c: complex128 = 1.0 + 2.0i;
  c as int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_invalid_cast_int_to_complex() {
    let input = r#"
fn test() -> complex128 {
  let x: int = 42;
  x as complex128
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_chained_cast() {
    let input = r#"
fn test() -> int {
  let x: int = 42;
  x as float64 as int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_redundant_cast() {
    let input = r#"
fn test() -> int {
  let x: int = 42;
  x as int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_integer_literal_overflow_int8() {
    let input = r#"
fn test() {
  let x: int8 = 1000;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_integer_literal_overflow_uint8() {
    let input = r#"
fn test() {
  let x: uint8 = 256;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_char_literal_overflow_uint8() {
    let input = r#"
fn test() {
  let x: uint8 = '中';
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_integer_literal_overflow_int16() {
    let input = r#"
fn test() {
  let x: int16 = 40000;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_negative_literal_overflow_int8() {
    let input = r#"
fn test() {
  let x: int8 = -129;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cannot_negate_unsigned() {
    let input = r#"
fn test() {
  let x: uint8 = -1;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cast_literal_overflow_int8() {
    let input = r#"
fn test() {
  let x = 255 as int8;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cast_literal_overflow_negative() {
    let input = r#"
fn test() {
  let x = (-129) as int8;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_bare_identifier_in_select_receive() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    let v = ch.receive() => v,
    _ => 0,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_none_pattern_in_select_receive() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    let None = ch.receive() => 0,
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_missing_some_arm() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      None => 0,
    },
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_missing_none_arm() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(v) => v,
    },
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_duplicate_some_arm() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(v) => v,
      Some(x) => x + 1,
      None => 0,
    },
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_duplicate_none_arm() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(v) => v,
      None => 0,
      None => 1,
    },
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_guard_not_allowed() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(v) if v > 0 => v,
      None => 0,
    },
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_multiple_select_receives() {
    let input = r#"
fn test() {
  let ch1 = Channel.new<int>();
  let ch2 = Channel.new<int>();
  select {
    let Some(v) = ch1.receive() => v,
    let Some(v) = ch2.receive() => v,
    _ => 0,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_invalid_pattern() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(v) => v,
      None => 0,
      _ => 1,
    },
    _ => 2,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_refutable_inner_pattern() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(1) => println("one"),
      None => println("closed"),
    },
    _ => println("timeout"),
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_shorthand_refutable_inner_pattern() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    let Some(1) = ch.receive() => println("one"),
    _ => println("timeout"),
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_select_match_empty_arms() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {},
    _ => println("timeout"),
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn break_value_type_mismatch_with_annotation() {
    let input = r#"
fn test() {
  let x: int = loop {
    break "hello"
  };
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_comparable_slice() {
    let input = r#"
fn test() {
  let a = [1, 2, 3];
  let b = [1, 2, 3];
  let result = a == b;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_comparable_function() {
    let input = r#"
fn test() {
  let f = |x: int| x + 1;
  let g = |x: int| x + 2;
  let result = f == g;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_comparable_struct_with_slice() {
    let input = r#"
struct Foo {
  items: Slice<int>,
}

fn test() {
  let a = Foo { items: [1, 2] };
  let b = Foo { items: [3, 4] };
  let result = a == b;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_not_comparable_enum_with_slice() {
    let input = r#"
enum Bar {
  Items(Slice<int>),
  Empty,
}

fn test() {
  let a = Bar.Items([1, 2]);
  let b = Bar.Empty;
  let result = a == b;
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_comparable_bound_rejects_slice() {
    let input = r#"
fn requires_comparable<T: Comparable>(_x: T) {}

fn test() {
  requires_comparable([1, 2, 3])
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_ordered_bound_rejects_bool() {
    let input = r#"
import "go:cmp"

fn requires_ordered<T: cmp.Ordered>(_x: T) {}

fn test() {
  requires_ordered(true)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_missing_bound_on_param_for_comparable() {
    let input = r#"
fn requires_comparable<T: Comparable>(_x: T) {}

fn wrapper<T>(x: T) {
  requires_comparable(x)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_missing_bound_on_param_for_ordered() {
    let input = r#"
import "go:cmp"

fn requires_ordered<T: cmp.Ordered>(_x: T) {}

fn wrapper<T>(x: T) {
  requires_ordered(x)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_propagated_bound_no_error() {
    let input = r#"
import "go:cmp"

fn requires_ordered<T: cmp.Ordered>(_x: T) {}

fn wrapper<T: cmp.Ordered>(x: T) {
  requires_ordered(x)
}
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_ordered_bound_allows_lt_in_body() {
    let input = r#"
import "go:cmp"

fn less<T: cmp.Ordered>(a: T, b: T) -> bool { a < b }
fn user() -> bool { less(1, 2) }
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_comparable_bound_allows_eq_in_body() {
    let input = r#"
fn eq<T: Comparable>(a: T, b: T) -> bool { a == b }
fn user() -> bool { eq(1, 1) }
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_shadowed_inner_generic_does_not_inherit_bound() {
    let input = r#"
import "go:cmp"

fn requires_ordered<T: cmp.Ordered>(_x: T) {}

struct Box<T> {}

impl<T: cmp.Ordered> Box<T> {
  fn bad<T>(self, x: T) { requires_ordered(x) }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_comparable_in_param_position_rejected() {
    let input = r#"
fn takes(_x: Comparable) {}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_ordered_in_param_position_rejected() {
    let input = r#"
import "go:cmp"

fn takes(_x: cmp.Ordered) {}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_ordered_satisfies_comparable_in_wrapper() {
    let input = r#"
import "go:cmp"

fn requires_comparable<T: Comparable>(_x: T) {}

fn wrapper<T: cmp.Ordered>(x: T) {
  requires_comparable(x)
}
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_comparable_does_not_satisfy_ordered_in_wrapper() {
    let input = r#"
import "go:cmp"

fn requires_ordered<T: cmp.Ordered>(_x: T) {}

fn wrapper<T: Comparable>(x: T) {
  requires_ordered(x)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_prelude_min_rejects_bool() {
    let input = r#"
fn test() -> bool {
  min(true, false)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_prelude_max_rejects_struct() {
    let input = r#"
struct Point { x: int, y: int }

fn test(a: Point, b: Point) -> Point {
  max(a, b)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_prelude_min_max_accepts_int_and_string() {
    let input = r#"
fn test_int() -> int { min(1, 2, 3) }
fn test_float() -> float64 { max(1.0, 2.0) }
fn test_string() -> string { min("a", "b", "c") }
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_prelude_min_max_accepts_user_cmp_ordered_bound() {
    let input = r#"
import "go:cmp"

fn pick<T: cmp.Ordered>(a: T, b: T) -> T { min(a, b) }
"#;
    let result = crate::_harness::infer::infer(input);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_impl_on_type_alias() {
    let source = r#"
type UserId = int

impl UserId {
  fn bump(self) -> int {
    self + 1
  }
}

fn main() {}
"#;
    assert_infer_error_snapshot!(source);
}

#[test]
fn infer_impl_on_foreign_type() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "ext",
        "lib.lis",
        r#"
pub struct Widget {
  pub name: string,
}
"#,
    );

    let source = r#"
import "ext"

impl ext.Widget {
  fn greet(self) -> string {
    "hello"
  }
}

fn main() {}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_non_pub_interface_with_pub_implementations() {
    let mut fs = MockFileSystem::new();

    let source = r#"
interface Shape {
  fn area(self) -> float64
  fn name(self) -> string
}

struct Circle {
  radius: float64
}

impl Circle {
  pub fn area(self) -> float64 {
    3.14159 * self.radius * self.radius
  }
  pub fn name(self) -> string {
    "Circle"
  }
}

fn main() {}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_non_pub_interface_with_private_implementations() {
    let input = r#"
import "go:fmt"

interface Greeter {
  fn greet(self) -> string
}

struct Hello { name: string }
impl Hello {
  fn greet(self) -> string { f"hello {self.name}" }
}

fn main() {
  let h = Hello { name: "world" }
  fmt.Println(h.greet())
}
"#;
    let result = crate::_harness::infer::infer(input);
    result.assert_no_errors();
}

#[test]
fn infer_unit_return_assigned_to_int() {
    let input = r#"
fn returns_unit() {}

fn test() {
  let x: int = returns_unit()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_unit_return_assigned_via_reassignment() {
    let input = r#"
fn returns_unit() {}

fn test() {
  let mut x = returns_unit()
  x = 42
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn lex_invalid_escape_sequence_in_string() {
    let input = r#"
fn main() {
  let s = "hello\!world"
}
"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn lex_invalid_escape_sequence_question_mark() {
    let input = r#"
fn main() {
  let s = "test\?string"
}
"#;
    assert_lex_error_snapshot!(input);
}

#[test]
fn infer_method_shadows_struct_field() {
    let input = r#"
struct Dog {
  name: string,
}

impl Dog {
  fn name(self) -> string {
    f"Dog:{self.name}"
  }
}

fn main() {
  let d = Dog { name: "Rex" }
  fmt.Println(d.name())
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_method_shadows_enum_field() {
    let input = r#"
enum Event<T> {
  Created { id: int, data: T },
  Updated { id: int, old_data: T, new_data: T },
  Deleted { id: int },
}

impl<T> Event<T> {
  fn id(self) -> int {
    match self {
      Event.Created { id, data: _ } => id,
      Event.Updated { id, old_data: _, new_data: _ } => id,
      Event.Deleted { id } => id,
    }
  }
}

fn main() {
  let evt = Event.Created { id: 42, data: "test" }
  fmt.Println(evt.id())
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_missing_constraint_on_generic_return_type() {
    let input = r#"
interface Displayable {
  fn display(self) -> string
}

struct Wrapper<T> {
  pub inner: T,
}

impl<T: Displayable> Wrapper<T> {
  pub fn show(self) -> string {
    f"[{self.inner.display()}]"
  }
}

pub fn wrap<T>(item: T) -> Wrapper<T> {
  Wrapper { inner: item }
}

fn main() {
  let w = wrap(42)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_missing_constraint_duplicate_bounds_deduped() {
    let input = r#"
pub interface Summable {
  fn value(self) -> int
}

struct Box<T: Summable> {
  val: T,
}

impl<T: Summable> Box<T> {
  fn new(v: T) -> Box<T> {
    Box { val: v }
  }

  fn map(self, f: fn(T) -> T) -> Box<T> {
    Box { val: f(self.val) }
  }
}

struct Num { n: int }
impl Num {
  pub fn value(self) -> int { self.n }
}

pub fn create_box<T>(item: T) -> Box<T> {
  Box { val: item }
}

fn main() {
  let b = create_box(Num { n: 5 })
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_self_reference_in_assignment() {
    let input = r#"
struct Node {
  next: Option<Ref<Node>>,
}

fn main() {
  let mut x = Node { next: None }
  x = Node { next: Some(&x) }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_panic_in_expression_position() {
    let input = r#"
fn main() {
  let x: int = panic("boom")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_redundant_pattern_with_all_literal_fields() {
    let input = r#"
struct Pt { x: int, y: int }

fn check(p: Pt) -> string {
  match p {
    Pt { x, y: 0 } => f"y=0, x={x}",
    Pt { x: 0, y: 0 } => "origin",
    Pt { x, y } => f"({x}, {y})",
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_ref_of_interface_type() {
    let input = r#"
interface Writable {
  fn write(self, data: string)
}

fn copy_data(dest: Ref<Writable>) {
  dest.write("hello")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_specialized_impl_interface_satisfaction_rejected() {
    let input = r#"
interface Describable {
  fn describe(self) -> string
}

struct Pair<A, B> { first: A, second: B }

impl Pair<int, string> {
  fn describe(self) -> string {
    f"Pair({self.first}, {self.second})"
  }
}

fn print_description(d: Describable) {
  d.describe()
}

fn main() {
  let p = Pair { first: 1, second: "hello" }
  print_description(p)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_duplicate_method_across_specialized_impls() {
    let input = r#"
struct Wrapper<T> {
  value: T,
}

impl Wrapper<int> {
  fn display(self) -> string {
    f"{self.value}"
  }
}

impl Wrapper<string> {
  fn display(self) -> string {
    self.value
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_self_type_in_interface() {
    let input = r#"
interface Comparable {
  fn compare(self, other: Self) -> int
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_empty_block_as_map() {
    let input = r#"
fn main() {
  let m: Map<string, int> = {}
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_method_without_parens() {
    let input = r#"
fn test(s: Slice<int>) -> int {
  s.length
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_immutable_arg_to_mut_param() {
    let input = r#"
fn sort(mut items: Slice<int>) {
  items = [1, 2, 3]
}

fn main() {
  let data = [3, 1, 2];
  sort(data)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_newtype_field_assignment() {
    let input = r#"
struct UserId(int)

fn main() {
  let mut n = UserId(1)
  n.0 = 2
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_map_field_chain_assignment() {
    let input = r#"
struct Point { x: int, y: int }

fn main() {
  let mut m = Map.new<string, Point>()
  m["a"] = Point { x: 1, y: 2 }
  m["a"].x = 5
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_map_field_chain_append() {
    let input = r#"
struct Outer { items: Slice<int> }

fn main() {
  let mut m = Map.new<string, Outer>()
  m["a"] = Outer{ items: [1] }
  m["a"].items.append(2)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_ref_slice_append() {
    let input = r#"
fn main() {
  let mut s = [1, 2, 3]
  let r: Ref<Slice<int>> = &s
  r.append(4)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_enum_field_type_conflict() {
    let input = r#"
enum Event {
  Click { target: int },
  Hover { target: string },
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_enum_field_type_conflict_struct_vs_tuple() {
    let input = r#"
enum Shape {
  Circle(string),
  Other { circle: int },
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_native_method_value() {
    let input = r#"
fn apply(f: fn(Slice<int>, VarArgs<int>) -> Slice<int>) {
  let _ = f
}

fn main() {
  apply(Slice.append)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_native_constructor_value() {
    let input = r#"
fn apply(f: fn() -> Channel<int>) {
  let _ = f
}

fn main() {
  apply(Channel.new)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_native_receiver_method_value() {
    let input = r#"
fn main() {
  let s = [1, 2, 3]
  let f = s.length
  let _ = f
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_private_method_expression() {
    let input = r#"
struct Box { value: int }

impl Box {
  fn add(self, x: int) -> int { self.value + x }
}

fn main() {
  let f = Box.add
  let _ = f
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_float_literal_int_cast() {
    let input = r#"
fn main() {
  let x = 3.14 as int
  let _ = x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_const_requires_simple_expression() {
    let input = r#"
const VALUE = {
  let x = 10
  x + 20
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_const_self_reference_cycle() {
    let input = r#"
const SELF = SELF
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_const_mutual_cycle() {
    let input = r#"
const A = B
const B = A
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_reference_to_scalar_const() {
    let input = r#"
const N = 42

fn bump(r: Ref<int>) {
  r.* = r.* + 1
}

fn main() {
  bump(&N)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_mutate_const_shows_const_hint() {
    let input = r#"
const N = 5

fn main() {
  N = 10
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_const_disallows_list_literal() {
    let input = r#"
const ITEMS = ["a", "b"]
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_const_disallows_tuple_literal() {
    let input = r#"
const PAIR = (1, 2)
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_const_disallows_struct_literal() {
    let input = r#"
struct Point { x: int, y: int }

const ORIGIN = Point { x: 0, y: 0 }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_complex_sub_expression() {
    let input = r#"
fn side_effect() -> int { 1 }

fn main() {
  let x = side_effect() + if true { 2 } else { 3 }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_complex_sub_expression_auto_address() {
    let input = r#"
struct Box {
  v: int,
}

impl Box {
  fn get(self: Ref<Box>) -> int {
    self.v
  }
}

fn make_box() -> Box { Box { v: 1 } }
fn side_effect() -> int { 1 }

fn main() {
  let _ = side_effect() + make_box().get()
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_complex_select_expression() {
    let input = r#"
fn main() {
  let ch1 = Channel.buffered<int>(1)
  let ch2 = Channel.buffered<int>(1)
  select {
    (if true { ch1 } else { ch2 }).send(1) => 0,
    _ => 1,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_reference_through_newtype() {
    let input = r#"
struct Wrap(int)

fn main() {
  let w = Wrap(1)
  let r = &w.0
  let _ = r
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_reference_through_newtype_nested() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn main() {
  let w = Wrap(Inner { x: 1 })
  let r = &w.0.x
  let _ = r
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_propagate_in_if_condition() {
    let input = r#"
fn check(s: string) -> Result<bool, string> {
  if s == "bad" { Err("bad") } else { Ok(true) }
}

fn run() -> Result<(), string> {
  if check("x")? {
    let _ = 1
  }
  Ok(())
}

fn main() { let _ = run() }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_propagate_in_while_condition() {
    let input = r#"
fn check(s: string) -> Result<bool, string> {
  if s == "bad" { Err("bad") } else { Ok(true) }
}

fn run() -> Result<(), string> {
  while check("x")? {
    let _ = 1
  }
  Ok(())
}

fn main() { let _ = run() }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_propagate_in_logical_and_rhs() {
    let input = r#"
fn check(s: string) -> Result<bool, string> {
  if s == "bad" { Err("bad") } else { Ok(true) }
}

fn run() -> Result<bool, string> {
  Ok(true && check("x")?)
}

fn main() { let _ = run() }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_propagate_in_logical_or_rhs() {
    let input = r#"
fn check(s: string) -> Result<bool, string> {
  if s == "bad" { Err("bad") } else { Ok(true) }
}

fn run() -> Result<bool, string> {
  Ok(false || check("x")?)
}

fn main() { let _ = run() }
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_failure_propagation_err_in_call_arg() {
    let input = r#"
fn f(x: int) -> int { x }

fn test() -> Result<int, string> {
  Ok(f(Err("e")?))
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_failure_propagation_none_in_binary() {
    let input = r#"
fn maybe() -> Option<int> {
  Some(1)
}

fn test() -> Option<int> {
  Some(maybe()? + None?)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_unsupported_bitwise_and() {
    let input = r#"
fn test() {
  let a = 1
  let b = 2
  let c = a & b
  let _ = c
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_unsupported_bitwise_or() {
    let input = r#"
fn test() {
  let a = 1
  let b = 2
  let c = a | b
  let _ = c
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_unsupported_bitwise_xor() {
    let input = r#"
fn test() {
  let a = 1
  let b = 2
  let c = a ^ b
  let _ = c
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_format_specifier_in_fstring() {
    let input = r#"
fn test() {
  let n = 255
  let s = f"hex: {n:02x}"
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_reserved_import_alias_go_keyword() {
    let input = r#"
import map "go:fmt"

fn test() {
  map.Println("hi")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_reserved_import_alias_predeclared() {
    let input = r#"
import nil "go:fmt"

fn test() {
  nil.Println("hi")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_reserved_import_alias_prelude() {
    let input = r#"
import Option "go:fmt"

fn test() {
  Option.Println("hi")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_reserved_import_alias_main() {
    let input = r#"
import main "go:fmt"

fn test() {
  main.Println("hi")
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail() {
    let input = r#"
fn test() -> int {
  let _ = 1
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail_in_if_branch() {
    let input = r#"
fn test(flag: bool) -> int {
  if flag {
    let _ = 1
  } else {
    2
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail_in_match_arm() {
    let input = r#"
fn test(x: int) -> int {
  match x {
    1 => { let _ = 1 },
    _ => 2,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail_assignment() {
    let input = r#"
fn test() -> int {
  let mut x = 0
  x = 1
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail_in_let_initializer() {
    let input = r#"
fn test() {
  let i = if true { { let _ = 1 } } else { 1 }
  let _ = i
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail_in_assignment_rhs() {
    let input = r#"
fn test() {
  let mut x = 0
  x = if true { { let _ = 1 } } else { 1 }
  let _ = x
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_statement_as_tail_in_break_payload() {
    let input = r#"
fn test() {
  let r: int = loop {
    break { let _ = 1 }
  }
  let _ = r
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_invalid_main_with_return_type() {
    let mut fs = MockFileSystem::new();
    let source = r#"
fn main() -> Result<(), string> {
  Ok(())
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);
    let result = compile_check(fs);
    assert!(
        !result.errors.is_empty(),
        "Expected error for main with return type"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("infer.invalid_main_signature")),
        "Expected invalid_main_signature error, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_invalid_main_with_params() {
    let mut fs = MockFileSystem::new();
    let source = r#"
fn main(x: int) {
  let _ = x
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);
    let result = compile_check(fs);
    assert!(
        !result.errors.is_empty(),
        "Expected error for main with params"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("infer.invalid_main_signature")),
        "Expected invalid_main_signature error, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_invalid_main_with_int_return() {
    let mut fs = MockFileSystem::new();
    let source = r#"
fn main() -> int {
  42
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);
    let result = compile_check(fs);
    assert!(
        !result.errors.is_empty(),
        "Expected error for main with int return"
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("infer.invalid_main_signature")),
        "Expected invalid_main_signature error, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_definition_shadows_go_import() {
    let mut fs = MockFileSystem::new();
    let source = r#"
import "go:fmt"

fn fmt() {}

fn main() {
  let _ = fmt.Println("hi")
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);
    let result = compile_check(fs);
    assert!(!result.errors.is_empty(), "Expected error");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("resolve.definition_shadows_import")),
        "Expected definition_shadows_import error, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_definition_shadows_local_import() {
    let mut fs = MockFileSystem::new();
    fs.add_file("lib", "mod.lis", "pub fn hello() -> int { 7 }");
    let source = r#"
import "lib"

fn lib() {}

fn main() {
  let _ = lib.hello()
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);
    let result = compile_check(fs);
    assert!(!result.errors.is_empty(), "Expected error");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("resolve.definition_shadows_import")),
        "Expected definition_shadows_import error, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_struct_shadows_import_alias() {
    let mut fs = MockFileSystem::new();
    fs.add_file("lib", "mod.lis", "pub fn hello() -> int { 7 }");
    let source = r#"
import util "lib"

struct util { x: int }

fn main() {
  let _ = util.hello()
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);
    let result = compile_check(fs);
    assert!(!result.errors.is_empty(), "Expected error");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("resolve.definition_shadows_import")),
        "Expected definition_shadows_import error, got: {:?}",
        result.errors
    );
}

#[test]
fn infer_builtin_as_value() {
    let input = r#"
fn main() {
  let f = imaginary
  let _ = f
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_tuple_struct_constructor_as_value() {
    let input = r#"
struct Point(int, int)

fn main() {
  let f = Point
  let _ = f
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_alias_tuple_struct_as_value() {
    let input = r#"
struct Point(int, int)
type P = Point

fn main() {
  let f = P
  let _ = f
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_record_struct_as_value() {
    let input = r#"
struct Coord { x: int, y: int }

fn main() {
  let c = Coord
  let _ = c
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_alias_record_struct_as_value() {
    let input = r#"
struct Coord { x: int, y: int }
type C = Coord

fn main() {
  let c = C
  let _ = c
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cross_module_record_struct_as_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub struct Point { x: int, y: int }
"#,
    );

    let source = r#"
import "util"

fn main() {
  let p = util.Point
  let _ = p
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_type_alias_as_qualifier_parameterized() {
    let input = r#"
type O<T> = Option<T>
fn main() {
  let f: fn(int) -> O<int> = O.Some
  let _ = f
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_type_alias_as_qualifier_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub struct Box {}

pub fn make<T>() -> Slice<T> {
  []
}
"#,
    );

    let source = r#"
import "util"

type B = util.Box

fn main() {
  let s: Slice<int> = B.make()
  let _ = s
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_parenthesized_type_qualifier() {
    let input = r#"
struct Box {}

impl Box {
  fn one() -> int { 1 }
}

fn main() {
  let n = (Box).one()
  let _ = n
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_parenthesized_module_qualifier() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub fn one() -> int { 1 }
"#,
    );

    let source = r#"
import "util"

fn main() {
  let n = (util).one()
  let _ = n
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_parenthesized_cross_module_type_qualifier() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub struct Box {}

impl Box {
  pub fn one() -> int { 1 }
}
"#,
    );

    let source = r#"
import "util"

fn main() {
  let n = (util.Box).one()
  let _ = n
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_nested_parenthesized_qualifier() {
    let input = r#"
struct Box {}

impl Box {
  fn one() -> int { 1 }
}

fn main() {
  let n = ((Box)).one()
  let _ = n
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_cross_module_tuple_struct_as_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub struct P(int, int)
"#,
    );

    let source = r#"
import "util"

fn main() {
  let f = util.P
  let _ = f
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_cross_module_generic_tuple_struct_as_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub struct W<T>(T)
"#,
    );

    let source = r#"
import "util"

fn main() {
  let f = util.W
  let _ = f
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_generic_alias_to_cross_module_tuple_struct_as_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "lib.lis",
        r#"
pub struct W<T>(T)
"#,
    );

    let source = r#"
import "util"

type G<T> = util.W<T>

fn main() {
  let f = G
  let _ = f
}
"#;
    fs.add_file("main", "main.lis", source);

    let result = infer_module("main", fs);
    assert_multimodule_infer_error_snapshot!(result, source);
}

#[test]
fn infer_propagate_on_partial() {
    let input = r#"
fn test() -> Result<int, string> {
  let p: Partial<int, string> = Partial.Ok(42)
  p?
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn match_non_exhaustive_partial() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  match p {
    Partial.Ok(n) => n,
    Partial.Err(_) => 0,
  }
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_spread_on_non_variadic() {
    let input = r#"
fn takes_int(x: int) {}

fn test(args: Slice<int>) {
  takes_int(..args)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_spread_missing_required_positional_arg() {
    let input = r#"
import url "go:net/url"

fn test(rest: Slice<string>) -> Result<string, error> {
  url.JoinPath(..rest)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_spread_on_type_conversion() {
    let input = r#"
type Callback = fn(string) -> int

fn test(rest: Slice<fn(string) -> int>) {
  Callback(..rest)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_spread_not_last_arg() {
    let input = r#"
fn test() { foo(..xs, y); }
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn infer_immutable_spread_to_mut_variadic_param() {
    let input = r#"
fn touch(mut items: VarArgs<int>) {
  let _ = items
}

fn main() {
  let data = [3, 1, 2]
  touch(..data)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn infer_immutable_args_to_mut_variadic_param() {
    let input = r#"
fn touch(x: int, mut ys: VarArgs<int>) -> int {
  let _ = ys
  x
}

fn main() {
  let a = 1
  let b = 2
  let c = 3
  let _ = touch(a, b, c)
}
"#;
    assert_infer_error_snapshot!(input);
}

#[test]
fn parse_backtick_in_expression_simple() {
    let input = r#"
fn main() {
  let x = `hello`
  let _ = x
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_backtick_in_expression_with_embedded_quote() {
    let input = r#"
fn main() {
  let x = `has "quote" inside`
  let _ = x
}
"#;
    assert_parse_error_snapshot!(input);
}

#[test]
fn parse_compound_assignment_invalid_target() {
    let input = r#"
fn main() {
  { 1 } -= 2;
}
"#;
    assert_parse_error_snapshot!(input);
}
