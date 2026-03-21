use crate::assert_emit_snapshot;

#[test]
fn integer_literal() {
    let input = r#"
fn test() -> int {
  42
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negative_integer() {
    let input = r#"
fn test() -> int {
  -10
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero() {
    let input = r#"
fn test() -> int {
  0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn boolean_true() {
    let input = r#"
fn test() -> bool {
  true
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn boolean_false() {
    let input = r#"
fn test() -> bool {
  false
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn float_literal() {
    let input = r#"
fn test() {
  3.14
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negative_float() {
    let input = r#"
fn test() {
  -0.5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn float_whole_number() {
    let input = r#"
fn test() {
  2.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn char_literal() {
    let input = r#"
fn test() {
  'a'
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn char_newline() {
    let input = r#"
fn test() {
  '\n'
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_literal() {
    let input = r#"
fn test() {
  "hello world"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_with_escapes() {
    let input = r#"
fn test() {
  "hello\nworld"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_of_ints() {
    let input = r#"
fn test() -> Slice<int> {
  [1, 2, 3]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_of_bools() {
    let input = r#"
fn test() -> Slice<bool> {
  [true, false, true]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn empty_slice() {
    let input = r#"
fn test() {
  let x: Slice<int> = [];
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn hex_literal() {
    let input = r#"
fn test() -> int {
  0xFF
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn hex_large() {
    let input = r#"
fn test() -> uint64 {
  0xC96C5795D7870F42
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn hex_max_u64() {
    let input = r#"
fn test() -> uint64 {
  0xFFFFFFFFFFFFFFFF
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn large_uint64_decimal_literal() {
    let input = r#"
fn test() -> uint64 {
  18446744073709551615
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn float_scientific() {
    let input = r#"
fn test() -> float64 {
  1.5e10
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn float_scientific_negative_exponent() {
    let input = r#"
fn test() -> float64 {
  3.14e-5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn float_max_f64() {
    let input = r#"
fn test() -> float64 {
  1.7976931348623157e+308
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn octal_literal() {
    let input = r#"
fn test() -> int {
  0o755
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn octal_uppercase() {
    let input = r#"
fn test() -> int {
  0O644
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn octal_legacy() {
    let input = r#"
fn test() -> int {
  0755
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_literal() {
    let input = r#"
fn test() -> int {
  0b1010
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_uppercase() {
    let input = r#"
fn test() -> int {
  0B11110000
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_with_underscores() {
    let input = r#"
fn test() -> int {
  0b1111_0000_1111_0000
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn min_int64() {
    let input = r#"
fn test() -> int {
  -9223372036854775808
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn imaginary_integer() {
    let input = r#"
fn test() -> complex128 {
  4i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn imaginary_float() {
    let input = r#"
fn test() -> complex128 {
  3.14i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn imaginary_zero() {
    let input = r#"
fn test() -> complex128 {
  0i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn complex_addition() {
    let input = r#"
fn test() -> complex128 {
  3 + 4i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn complex_subtraction() {
    let input = r#"
fn test() -> complex128 {
  10 - 2i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn complex_multiplication() {
    let input = r#"
fn test() -> complex128 {
  (3 + 4i) * 2i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn imaginary_addition() {
    let input = r#"
fn test() -> complex128 {
  4i + 4i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negative_int8_literal() {
    let input = r#"
fn test() -> int8 {
  let x: int8 = -1;
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negative_float32_literal() {
    let input = r#"
fn test() -> float32 {
  let f: float32 = -1.5;
  f
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn null_escape_in_string() {
    let input = r#"
fn test() -> string {
  "a\0b"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn null_escape_in_char() {
    let input = r#"
fn test() -> rune {
  '\0'
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unicode_escape_in_string() {
    let input = r#"
fn test() -> string {
  "\u{1F600}"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unicode_escape_bmp_in_string() {
    let input = r#"
fn test() -> string {
  "\u{00E9}"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_display_format() {
    let input = r#"
import "go:fmt"

fn main() {
  let t = (1, "hello")
  fmt.Println(t)
  fmt.Println(f"tuple: {t}")
}
"#;
    assert_emit_snapshot!(input);
}
