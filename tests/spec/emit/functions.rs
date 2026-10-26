use crate::assert_emit_snapshot;

#[test]
fn simple_function() {
    let input = r#"
fn foo() {
  let x = 42;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_with_parameters() {
    let input = r#"
fn add(a: int, b: int) {
  let sum = a + b;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_with_return_type() {
    let input = r#"
fn get_value() -> int {
  42
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_with_params_and_return() {
    let input = r#"
fn add(a: int, b: int) -> int {
  a + b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_returning_bool() {
    let input = r#"
fn is_positive(x: int) -> bool {
  x > 0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_identity() {
    let input = r#"
fn identity<T>(x: T) -> T {
  x
}

fn test() -> int {
  identity(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_two_params() {
    let input = r#"
fn first<A, B>(a: A, b: B) -> A {
  a
}

fn test() -> int {
  first(10, true)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn method_with_reference_self() {
    let input = r#"
struct Counter { count: int }

impl Counter {
  fn get_count(self: Ref<Counter>) -> int {
    self.count
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn multiple_methods() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn new(x: int, y: int) -> Point {
    Point { x: x, y: y }
  }

  fn get_x(self: Point) -> int {
    self.x
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn static_method_call() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn new(x: int, y: int) -> Point {
    Point { x: x, y: y }
  }
}

fn main() -> int {
  let p = Point.new(1, 2);
  p.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn simple_closure() {
    let input = r#"
fn test() {
  let add_one = |x: int| x + 1;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_with_multiple_params() {
    let input = r#"
fn test() {
  let add = |a: int, b: int| a + b;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_with_assignment_body() {
    let input = r#"
fn test() {
  let mut count = 0;
  let inc = || { count = count + 1; };
  inc();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_param_shadow() {
    let input = r#"
fn test() -> int {
  let f = |x: int| -> int {
    let x = x + 10
    x
  }
  f(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_let_shadow() {
    let input = r#"
fn test() -> string {
  let f = || -> string {
    let x = 1
    let x = f"was {x}"
    x
  }
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_param_shadow_outer_access() {
    let input = r#"
fn test() -> string {
  let x = "outer"
  let f = |x: int| -> string {
    let x = x * 2
    f"doubled: {x}"
  }
  let result = f(5)
  f"{result} and {x}"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_let_if_reads_the_captured_local_it_shadows() {
    let input = r#"
fn run(x: int) -> int {
  let f = |c: bool| {
    let x = if c { x + 1 } else { 0 }
    x * 2
  }
  f(true)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_let_match_reads_the_captured_subject_it_shadows() {
    let input = r#"
fn run(x: Option<int>) -> int {
  let f = |k: int| {
    let x = match x {
      Some(v) => v + k,
      None => 0,
    }
    x * 2
  }
  f(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_let_if_writes_the_captured_local_it_shadows() {
    let input = r#"
fn run(start: int) -> int {
  let mut total = start
  let f = |c: bool| {
    let total = if c {
      total = total + 1
      total
    } else {
      0
    }
    total * 2
  }
  let r = f(true)
  r + total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_let_else_or_pattern_reads_the_captured_local_it_shadows() {
    let input = r#"
enum E { A(int), B(int), C }

fn run(x: int, e: E) -> int {
  let f = || {
    let E.A(x) | E.B(x) = e else { return x }
    x
  }
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_shadowing_while_let_keeps_captured_assignment() {
    let input = r#"
fn main() {
  let mut x = 10
  let f = || {
    let mut next = Some(2)
    while let Some(x) = next {
      let read = || x
      next = None
      if read() != 2 { panic("loop capture") }
    }
    x += 1
    x
  }
  if f() != 11 || x != 11 { panic("outer capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_shadowing_if_let_and_for_keeps_outer_read() {
    let input = r#"
fn main() {
  let x = 7
  let f = || {
    let mut total = 0
    if let Some(x) = Some(x + 1) {
      for x in [x, x + 1] {
        let read = || x
        total += read()
      }
    }
    total + x
  }
  if f() != 24 { panic("shadowed capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_shadowing_select_and_let_else_keeps_failure_capture() {
    let input = r#"
enum Choice { A(int), B(int), Empty }

fn main() {
  let x = 13
  let ch = Channel.buffered<int>(1)
  ch.send(4)
  let f = || {
    let selected = select {
      let Some(x) = ch.receive() => {
        let read = || x
        read()
      },
      _ => 0,
    }
    let Choice.A(x) | Choice.B(x) = Choice.Empty else {
      return selected + x
    }
    x
  }
  if f() != 17 { panic("failure capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_fold_accumulator_keeps_captured_source() {
    let input = r#"
fn main() {
  let x = 7
  let f = || [x].fold(0, |x, y| x + y)
  if f() != 7 { panic("captured source") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_closure_fold_accumulator_keeps_captured_source() {
    let input = r#"
fn main() {
  let x = 7
  let f = || {
    let g = || {
      let x = [x, x + 1].fold(0, |x, y| x + y)
      x
    }
    g() + x
  }
  if f() != 22 || x != 7 { panic("nested capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_branch_fold_accumulator_keeps_captured_source() {
    let input = r#"
fn main() {
  let x = 7
  let f = || {
    if x > 0 {
      let x = [x, x + 1].fold(0, |x, y| x + y)
      x
    } else { x }
  }
  if f() != 15 || x != 7 { panic("branch capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_call_no_args() {
    let input = r#"
fn get_value() -> int {
  42
}

fn test() -> int {
  get_value()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_call_with_args() {
    let input = r#"
fn add(a: int, b: int) -> int {
  a + b
}

fn test() -> int {
  add(10, 20)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_function_calls() {
    let input = r#"
fn double(x: int) -> int {
  x * 2
}

fn test() -> int {
  double(double(5))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_function() {
    let input = r#"
fn factorial(n: int) -> int {
  if n <= 1 {
    1
  } else {
    n * factorial(n - 1)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn method_call() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn get_x(self: Point) -> int {
    self.x
  }
}

fn test(p: Point) -> int {
  p.get_x()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn numeric_explicit_int_to_float64() {
    let input = r#"
fn get_int() -> int {
  42
}

fn accept_float(x: float64) -> float64 {
  x
}

fn test() -> float64 {
  accept_float(get_int() as float64)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn numeric_int_literal_to_float64() {
    let input = r#"
fn accept_float(x: float64) -> float64 {
  x
}

fn test() -> float64 {
  accept_float(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn numeric_int_literals_to_mixed_params() {
    let input = r#"
fn mixed_params(a: float64, b: int, c: float64) -> float64 {
  a + c
}

fn test() -> float64 {
  mixed_params(1, 2, 3)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn numeric_explicit_float64_to_int() {
    let input = r#"
fn get_float() -> float64 {
  3.14
}

fn test() -> int {
  get_float() as int
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn numeric_explicit_float64_to_float32() {
    let input = r#"
fn get_float64() -> float64 {
  3.14
}

fn test() -> float32 {
  get_float64() as float32
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_float_var_to_int() {
    let input = r#"
fn test() -> int {
  let f = 3.14
  f as int
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_through_a_type_alias_drops_the_conversion() {
    let input = r#"
type Count = int

fn total(n: Count) -> int {
  n as int
}

fn tally(n: int) -> Count {
  n as Count
}

fn test() {
  if total(4) != 4 { panic("an alias cast should return its argument") }
  if tally(5) != 5 { panic("an alias target should return its argument") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_to_a_named_type_over_the_same_underlying_keeps_the_conversion() {
    let input = r#"
struct Meters(int)

fn measure(n: int) -> Meters {
  n as Meters
}

fn test() {
  if measure(6) != Meters(6) { panic("a named type should keep its conversion") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_of_a_constant_through_an_alias_keeps_the_conversion() {
    let input = r#"
type Count = int

fn pinned() -> Count {
  1 as Count
}

fn test() {
  if pinned() != 1 { panic("a pinned constant should keep its value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_in_complex_expression() {
    let input = r#"
fn test() -> float64 {
  let a: int = 3;
  let b: int = 4;
  let c: int = 2;
  ((a + b) as float64) / (c as float64)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_of_a_parenthesized_operand_emits_one_pair() {
    let input = r#"
fn gray(r: int, g: int) -> uint8 {
  ((r / 256 + g / 256) / 2) as uint8
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_and_bool_constants_emit_untyped() {
    let input = r#"
const NAME = "app"
const DEBUG = true
const LIMIT = 3

fn label() -> string {
  if DEBUG { NAME } else { "" }
}

fn cap() -> int { LIMIT }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_string_to_byte_slice() {
    let input = r#"
fn test() -> Slice<byte> {
  "hello" as Slice<byte>
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_byte_slice_to_string() {
    let input = r#"
fn test() -> string {
  let bytes: Slice<byte> = "hello" as Slice<byte>;
  bytes as string
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_int_to_rune() {
    let input = r#"
fn test() -> rune {
  65 as rune
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_rune_to_string() {
    let input = r#"
fn test() -> string {
  'A' as string
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_precedence_with_addition() {
    let input = r#"
fn test() -> float64 {
  let a: int = 1;
  let b: int = 2;
  a as float64 + b as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_to_float_pins_integer_type() {
    let input = r#"
fn heal(level: int, potency: float64) -> float64 {
  ((2 << level) as float64) * potency
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_nested_shift_to_float_pins_integer_type() {
    let input = r#"
fn scale(level: int) -> float64 {
  ((2 << level) + 3) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_variable_shift_to_float_needs_no_pin() {
    let input = r#"
fn scale(base: int, level: int) -> float64 {
  (base << level) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_constant_shift_to_float_needs_no_pin() {
    let input = r#"
fn scale() -> float64 {
  (2 << 3) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_with_constant_arithmetic_base_pins() {
    let input = r#"
fn scale(level: int) -> float64 {
  ((1 + 1) << level) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_constant_shift_with_arithmetic_count_needs_no_pin() {
    let input = r#"
fn scale() -> float64 {
  (1 << (50 + 50)) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_with_const_count_needs_no_pin() {
    let input = r#"
const SHIFT = 60 + 8

fn scale() -> float64 {
  (1 << SHIFT) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_with_local_const_count_needs_no_pin() {
    let input = r#"
fn scale() -> float64 {
  const SHIFT = 60 + 8
  (1 << SHIFT) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_imported_const_shifted_operand_pins() {
    let input = r#"
import "go:math"

fn scale(level: int) -> float64 {
  (math.MaxInt8 << level) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_forward_alias_const_stays_const() {
    let input = r#"
const SHIFT = LATER
const LATER = 60 + 8

fn scale() -> float64 {
  (1 << SHIFT) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_shift_imported_const_count_no_pin_but_var_count_pins() {
    let input = r#"
import "go:math"
import "go:runtime"

fn const_count() -> float64 {
  (1 << math.MaxInt8) as float64
}

fn var_count() -> float64 {
  (1 << runtime.MemProfileRate) as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn float32_parameter_and_return() {
    let input = r#"
fn accept_float32(x: float32) -> float32 {
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_type_as_parameter() {
    let input = r#"
fn apply(f: fn(int) -> int, x: int) -> int {
  f(x)
}

fn double(x: int) -> int { x * 2 }

fn test() -> int {
  apply(double, 21)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_type_with_mut_parameter() {
    let input = r#"
fn run(items: mut Slice<int>, action: fn(mut Slice<int>) -> ()) {
  action(items)
}

fn bump(items: mut Slice<int>) {
  items[0] += 1
}

fn test() {
  let mut data = [0]
  run(data, bump)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn lambda_with_mut_parameter() {
    let input = r#"
fn apply(f: fn(mut Slice<int>) -> (), xs: mut Slice<int>) {
  f(xs)
}

fn test() {
  let bump = |xs: mut Slice<int>| {
    xs[0] += 1
  }
  let mut data = [41]
  apply(bump, data)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_type_as_return() {
    let input = r#"
fn make_adder(n: int) -> fn(int) -> int {
  |x: int| x + n
}

fn test() -> int {
  let add5 = make_adder(5);
  add5(10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn higher_order_function() {
    let input = r#"
fn compose(f: fn(int) -> int, g: fn(int) -> int) -> fn(int) -> int {
  |x: int| f(g(x))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_in_lambda() {
    let input = r#"
fn fallible() -> Result<int, string> {
  Ok(10)
}

fn test() -> int {
  let f = |x: int| -> Result<int, string> {
    let v = fallible()?;
    Ok(v + x)
  };
  match f(5) {
    Ok(n) => n,
    Err(_) => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_pattern_in_function() {
    let input = r#"
fn sum_pair(pair: Slice<int>) -> int {
  match pair {
    [a, b] => a + b,
    _ => 0,
  }
}

fn test() -> int {
  sum_pair([10, 20])
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_method_name_map() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn map(self: Box<T>, f: fn(T) -> T) -> Box<T> {
    Box { value: f(self.value) }
  }
}

fn test() -> int {
  let b = Box { value: 10 };
  b.map(|x: int| x * 2).value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_variable_name() {
    let input = r#"
fn test() -> int {
  let map = 42;
  let range = 10;
  map + range
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_function_parameter() {
    let input = r#"
fn use_map(map: int, range: int) -> int {
  map + range
}

fn test() -> int {
  use_map(10, 20)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_lambda_parameter() {
    let input = r#"
fn test() -> int {
  let f = |map: int, range: int| map + range;
  f(10, 20)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_for_loop_binding() {
    let input = r#"
fn test() -> int {
  let mut total = 0;
  for range in [1, 2, 3] {
    total += range;
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_match_binding() {
    let input = r#"
fn test() -> int {
  let x = Some(42);
  match x {
    Some(map) => map,
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_function_name() {
    let input = r#"
fn map(x: int) -> int {
  x * 2
}

fn range(x: int) -> int {
  x + 1
}

fn test() -> int {
  map(5) + range(10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_struct_field_assignment() {
    let input = r#"
struct Config {
  range: int,
  default: string,
}

fn main() {
  let mut c = Config { range: 10, default: "hello" }
  c.range = 20
  c.default = "world"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_go_keyword_type_name() {
    let input = r#"
import "go:fmt"

struct default {
  value: int,
}

fn main() {
  let x = default { value: 42 }
  fmt.Println(x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_keyword_generic_parameter() {
    let input = r#"
import "go:fmt"

fn id<range>(x: range) -> range {
  x
}

fn main() {
  fmt.Println(id(7))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_predeclared_generic_parameter() {
    let input = r#"
import "go:fmt"

fn f<int>(x: int) -> int {
  let xs = [1, 2, 3]
  let _ = xs.length()
  x
}

fn main() {
  fmt.Println(f("hello"))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_constructor_with_predeclared_generic_parameter() {
    let input = r#"
import "go:fmt"

enum Box<int> {
  Some(int),
  None,
}

fn main() {
  let b = Box.Some("x")
  match b {
    Box.Some(v) => fmt.Println(v),
    Box.None => fmt.Println("none"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_go_predeclared_type_name_len() {
    let input = r#"
import "go:fmt"

struct len {
  v: int,
}

fn main() {
  let xs = [1, 2, 3]
  fmt.Println(xs.length())
  let l = len { v: 1 }
  fmt.Println(l.v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_go_predeclared_type_name_iota_with_enum() {
    let input = r#"
import "go:fmt"

struct iota {
  v: int,
}

enum Color {
  Red,
  Green,
}

fn main() {
  let i = iota { v: 1 }
  let c = Color.Red
  match c {
    Color.Red => fmt.Println("red"),
    Color.Green => fmt.Println("green"),
  }
  fmt.Println(i.v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_tag_constant_spells_reserved_word() {
    let input = r#"
import "go:fmt"

enum ma {
  ke,
  other,
}

fn main() {
  let m = ma.ke
  let mut counts = Map.new<string, int>()
  counts["a"] = 1
  match m {
    ma.ke => fmt.Println(counts.length()),
    ma.other => fmt.Println("other"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pub_function_uncased_name_gets_export_prefix() {
    let input = r#"
import "go:fmt"

pub fn 挨拶() -> string {
  "こんにちは"
}

fn main() {
  fmt.Println(挨拶())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_pattern_param_public_field() {
    let input = r#"
struct Point { pub x: int, pub y: int }

fn get_x(Point { x, .. }: Point) -> int {
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_function() {
    let input = r#"
/// Adds two integers together.
fn add(a: int, b: int) -> int {
  a + b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_multiline() {
    let input = r#"
/// Calculates the factorial of a number.
///
/// Returns 1 for n <= 1.
fn factorial(n: int) -> int {
  if n <= 1 { 1 } else { n * factorial(n - 1) }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn panic_as_value_expression() {
    let input = r#"
fn panicky() -> int {
  panic("oh no!")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_returning_user_function_in_body() {
    let input = r#"
fn fail(msg: string) -> Never { panic(msg) }

fn test() -> string {
  fail("boom")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_alias_call_diverges() {
    let input = r#"
type Early = Halt
type Halt = Never

fn die() -> Early { panic("dead") }

fn pick(n: int) -> int {
  match n {
    0 => die(),
    _ => n,
  }
}

fn main() {
  let x: int = if pick(2) > 0 { 1 } else { die() }
  if x != 1 { panic("wrong branch") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_typed_let_shadowing_same_block_binding_takes_fresh_name() {
    let input = r#"
fn fail(msg: string) -> Never { panic(msg) }

fn test(n: int) {
  let x = n + 1
  let _ = x
  let x: int = fail("boom")
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_bodied_lambda_into_unknown_emits_unit_return() {
    let input = r#"
fn take_any(x: Unknown) {}

fn test() {
  take_any(|| { panic("boom") })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_bodied_lambda_into_generic_keeps_struct_return() {
    let input = r#"
fn run<T>(f: fn() -> T) -> int {
  let _ = f
  0
}

fn test() -> int {
  run(|| { panic("boom") })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_returning_function_matches_its_function_type() {
    let input = r#"
fn die() -> Never { panic("x") }

fn call(f: fn() -> Never) { f() }

fn named() { call(die) }

fn lambda() { call(|| { die() }) }

fn typed_lambda() {
  let g: fn() -> Never = || { panic("z") }
  call(g)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_returning_function_value_into_generic_result_gets_struct_return() {
    let input = r#"
fn die() -> Never { panic("x") }

fn run<T>(f: fn() -> T) -> int {
  let _ = f
  0
}

fn test() -> int {
  run(die)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn never_returning_function_value_into_concrete_generic_result_gets_slot_return() {
    let input = r#"
fn die() -> Never { panic("x") }

fn apply<T>(f: fn() -> T) -> T {
  f()
}

struct Box { n: int }

impl Box {
  fn run<T>(self, f: fn() -> T) -> T {
    f()
  }
}

struct Holder<T> {
  f: fn() -> T,
}

fn free() -> int {
  apply(die)
}

fn ufcs(b: Box) -> int {
  b.run(die)
}

fn enclosing<U>() -> U {
  apply(die)
}

fn field() -> Holder<int> {
  Holder { f: die }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_public_function() {
    let input = r#"
/// A publicly exported function.
pub fn greet(name: string) -> string {
  f"Hello, {name}!"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pipeline_with_builtin_slice_length() {
    let input = r#"
fn test() -> int {
  let items = [1, 2, 3];
  items |> Slice.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn init_function_escaped() {
    let input = r#"
fn init() -> int {
  42
}

fn test() -> int {
  init()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_return_only_type_param() {
    let input = r#"
fn default_value<T>() -> Option<T> {
  None
}

fn test() -> Option<int> {
  default_value()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_interface_type_param_in_match() {
    let input = r#"
pub interface Printable {
  fn to_string() -> string
}

struct Box { label: string }
impl Box {
  pub fn to_string(self) -> string { self.label }
}

struct Circle { radius: int }
impl Circle {
  pub fn to_string(self) -> string { "circle" }
}

fn find_it(n: int) -> Option<Printable> {
  match n {
    1 => Some(Box { label: "found" }),
    2 => Some(Circle { radius: 10 }),
    _ => None,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_bind_unit_returning_call() {
    let input = r#"
fn do_nothing() {}

fn test() {
  let r = do_nothing()
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_named_any_renamed() {
    let input = r#"
fn any<T>(items: Slice<T>, pred: fn(T) -> bool) -> bool {
  for item in items {
    if pred(item) { return true }
  }
  false
}

fn test() -> bool {
  any([1, 2, 3], |x| x > 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_with_map_key_type_parameter() {
    let input = r#"
fn put_in_map<K: Comparable, V>(key: K, value: V) -> Map<K, V> {
  let mut m: mut Map<K, V> = Map.new()
  m[key] = value
  m
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct_with_map_field_comparable_constraint() {
    let input = r#"
struct Cache<K: Comparable, V> {
  data: Map<K, V>,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_interface_method_err_result_type() {
    let input = r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  let err = ctx.Err()
  fmt.Println(f"err={err}")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_collision_with_range_loop_variable() {
    let input = r#"
import "go:fmt"

struct IntList {
  items: Slice<int>,
}

impl IntList {
  fn display(self) -> string {
    let mut s = "["
    for i in 0..self.items.length() {
      if i > 0 {
        s = s + ", "
      }
      let item = self.items[i]
      s = s + f"{item}"
    }
    s + "]"
  }
}

fn main() {
  let list = IntList { items: [1, 2, 3] }
  fmt.Println(list.display())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_none_for_interface_parameter() {
    let input = r#"
import "go:net/http"
import "go:fmt"

fn main() {
  let req = http.NewRequest("GET", "https://example.com", None)
  fmt.Println(req)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_none_before_call_argument_stays_inline() {
    let input = r#"
import "go:fmt"
import "go:go/types"
import "go:strings"

fn main() {
  fmt.Println(types.Id(None, strings.ToUpper("x")))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_some_for_interface_parameter() {
    let input = r#"
import "go:net/http"
import "go:strings"
import "go:fmt"

fn main() {
  let mut body = strings.NewReader("hello")
  let req = http.NewRequest("POST", "https://example.com", Some(body))
  fmt.Println(req)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_with_mut_parameter() {
    let input = r#"
fn process(items: mut Slice<int>) {
  items[0] = 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_with_unused_destructured_param() {
    let input = r#"
fn foo((a, b): (int, int)) -> int {
  1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_fn_to_type_alias() {
    let input = r#"
type Handler = fn(int) -> string

fn my_handler(x: int) -> string {
  "ok"
}

fn test() -> Handler {
  my_handler as Handler
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_type_alias_to_fn() {
    let input = r#"
type Handler = fn(int) -> string

fn my_handler(x: int) -> string {
  "ok"
}

fn test() -> fn(int) -> string {
  let h = my_handler as Handler
  h as fn(int) -> string
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_tuple_struct_to_underlying() {
    let input = r#"
struct Wrapper(int)

fn test() -> int {
  let w = Wrapper(42)
  w as int
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_underlying_to_tuple_struct() {
    let input = r#"
struct Wrapper(int)

fn test() -> Wrapper {
  let n = 42
  n as Wrapper
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_key_generic_in_expression_comparable() {
    let input = r#"
fn foo<U>(u: U) {}

fn make_map<T: Comparable>(t: T) -> int {
  foo(Map.new<T, int>())
  0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn complex_param_pattern_no_name_collision() {
    let input = r#"
fn f(arg_1: int, (x, y): (int, int)) -> int { arg_1 + x + y }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_unit_function_preserves_side_effects() {
    let input = r#"
fn foo() {
  let _ = 1
}

fn main() {
  return foo()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_float_var_to_integer_type_alias() {
    let input = r#"
type MyInt = int

fn main() {
  let f = 3.14
  let x: MyInt = f as MyInt
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_multi_return_tuple_destructuring_shadow() {
    let input = r#"
import "go:math"

fn main() {
  let x = "hi"
  let _ = x
  let (x, y) = math.Modf(1.25)
  let _ = (x, y)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_multi_return_rhs_uses_shadowed_binding() {
    let input = r#"
import "go:math"

fn main() {
  let x = 1.25
  let (x, y) = math.Modf(x)
  let _ = (x, y)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_unit_function_block_side_effects() {
    let input = r#"
import "go:fmt"

fn side() -> int { fmt.Println("side"); 1 }

fn main() {
  return {
    let _ = side()
    ()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_fn_value_shadowed_wrapping() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let f = strconv.Atoi
  let f = strconv.Atoi
  let r = f("42")
  fmt.Println(r)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_fn_value_scope_isolation() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  {
    let f = strconv.Atoi
    let _ = f("1")
  }
  let f = |s: string| -> int { 0 }
  let r = f("2")
  fmt.Println(r)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_callable_shadow_does_not_inherit_global_abi() {
    let input = r#"
fn apply(x: int) -> Result<int, error> { Ok(x) }

fn main() {
  let apply = |x: int| -> int { x + 1 }
  let value = apply(1)
  let _ = value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_fn_value_if_binding_wrapping() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let cond = true
  let f = if cond { strconv.Atoi } else { strconv.Atoi }
  let r = f("99")
  fmt.Println(r)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_nullable_return_discarded() {
    let input = r#"
import "go:html/template"

fn main() {
  let t = template.New("x")
  t.Lookup("y")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_map_key_in_task_body() {
    let input = r#"
fn use_map_in_task<K: Comparable, V>(key: K, value: V) {
  task {
    let mut m: mut Map<K, V> = Map.new()
    m[key] = value
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_fn_value_parenthesized_call() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let r = (strconv.Atoi)("42")
  fmt.Println(r)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_variable_stored_constructor() {
    let input = r#"
enum List {
  Nil,
  Cons(int, List),
}

fn main() {
  let make = List.Cons
  let xs = make(1, List.Nil)
  let _ = xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_nullable_return_in_return_statement() {
    let input = r#"
import "go:flag"

fn get(name: string) -> Option<Ref<flag.Flag>> {
  flag.Lookup(name)
}

fn main() {
  let _ = get("x");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_fn_value_parenthesized_binding() {
    let input = r#"
import "go:strconv"

fn main() {
  let f: fn(string) -> Result<int, error> = (strconv.Atoi)
  let r = f("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_method_value_receiver_hoisted() {
    let input = r#"
import "go:bytes"
import "go:fmt"

fn make(counter: mut Ref<int>) -> mut Ref<bytes.Buffer> {
  counter.* = counter.* + 1
  bytes.NewBufferString("a\nb\n")
}

fn use_fn(f: fn(uint8) -> Partial<string, error>) {
  let _ = f(10)
  let _ = f(10)
}

fn main() {
  let mut count = 0
  use_fn(make(&count).ReadString)
  fmt.Println(count)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_tuple_partial_fn_value_adapts_to_lowered_slot() {
    let input = r#"
import "go:mime"

fn use_fn(f: fn(string) -> Partial<(string, Map<string, string>), error>) {
  let _ = f("text/plain")
}

fn main() {
  use_fn(mime.ParseMediaType)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_value_instantiated() {
    let input = r#"
fn id<T>(x: T) -> T { x }

fn main() {
  let f = id
  let _ = f(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_call_private_keyword_method() {
    let input = r#"
struct Box { x: int }

impl Box {
  fn range(self) -> int { self.x }
  fn chan(self) -> int { self.x }
}

fn main() {
  let b = Box { x: 1 }
  let _ = Box.range(b)
  let _ = Box.chan(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_import_pointer_receiver_method_expression() {
    let input = r#"
import "go:bytes"

fn main() {
  let f = bytes.Buffer.ReadString
  let mut buf = bytes.Buffer {}
  let _ = f(&buf, 10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_import_alias_pointer_receiver_method_expression() {
    let input = r#"
import "go:bytes"

type Buffer = bytes.Buffer

fn main() {
  let f = Buffer.ReadString
  let mut buf = Buffer {}
  let _ = f(&buf, 10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_alias_method_value_and_struct_pattern() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn get(self) -> T { self.value }
}

type IntBox = Box<int>

fn test(b: IntBox) -> int {
  let get = IntBox.get
  let IntBox { value } = b
  get(b) + value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_function_ref_first_param_not_pointer_receiver() {
    let input = r#"
import "go:flag"

fn main() {
  let f = flag.UnquoteUsage
  let fl = flag.Lookup("verbose")
  match fl {
    Some(v) => { let _ = f(v) },
    None => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_variadic_fn_value_wrapper_spreads_args() {
    let input = r#"
import fmt "go:fmt"
import os "go:os"

fn main() {
  let f = fmt.Fprintf
  let _ = f(os.Stdout, "hi %d", 3)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_callback_result_adaptation() {
    let input = r#"
import "go:flag"

fn main() {
  flag.Func("verbose", "enable verbose", |_val: string| -> Result<(), error> {
    Ok(())
  })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_callback_bound_to_aliased_fn_type() {
    let input = r#"
import "go:path/filepath"
import "go:io/fs"

fn main() {
  let walker: filepath.WalkFunc = |_path: string, _info: fs.FileInfo, _err: error| -> Result<(), error> {
    Ok(())
  }
  let _ = filepath.Walk("/tmp", walker)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_as_function_argument() {
    let input = r#"
fn noop() {}
fn take(_x: ()) {}

fn test() {
  take(noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_tuple_struct_constructor() {
    let input = r#"
fn noop() {}
struct P((), int)

fn test() -> P {
  P(noop(), 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_ufcs_method_arg() {
    let input = r#"
fn noop() {}

struct Box {}

impl Box {
  fn ping(self, _x: ()) {}
}

fn test() {
  let b = Box {}
  Box.ping(b, noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_call_types_a_literal_of_another_default() {
    let input = r#"
fn ident<T>(v: T) -> T { v }

fn test() -> float64 {
  let widened: float64 = ident(1)
  let already: float64 = ident(1.5)
  let narrow: float32 = ident(2)
  widened / already + narrow as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_call_types_a_negated_literal() {
    let input = r#"
fn ident<T>(v: T) -> T { v }

fn test() -> float64 {
  let parenthesized: float64 = -(1)
  let negated: float64 = ident(-(1))
  parenthesized / 2.0 + negated / 2.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn method_call_types_a_literal_of_another_default() {
    let input = r#"
struct Box { label: string }

impl Box {
  fn scale<T>(self, v: T) -> T { v }
}

fn test() -> byte {
  let b = Box { label: "x" }
  let widened: float32 = b.scale(1)
  let letter: byte = b.scale('a')
  letter + widened as byte
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_call_leaves_a_literal_in_a_concrete_slot_alone() {
    let input = r#"
fn scaled<T>(v: T, _factor: float64) -> T { v }
fn work() -> Result<int, error> { Ok(1) }

fn test() -> int {
  let r = work()
  let t: (Result<int, error>, int) = (scaled(r, 1), 0)
  t.1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_call_types_a_literal_into_a_newtype() {
    let input = r#"
struct Flag(bool)
struct Label(string)
struct Ticket(int)
struct Weight(float64)
struct Letter(rune)

fn ident<T>(v: T) -> T { v }

fn test() -> Flag {
  let label: Label = ident("x")
  let ticket: Ticket = ident(1)
  let weight: Weight = ident(1.5)
  let letter: Letter = ident('a')
  let _ = (label, ticket, weight, letter)
  ident(true)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_call_leaves_a_constant_alone_when_a_typed_sibling_binds_the_parameter() {
    let input = r#"
fn pick<T>(_a: T, b: T) -> T { b }
fn ident<T>(v: T) -> T { v }

fn test(w: int64, f: float64) -> float64 {
  let a: int64 = pick(w, 1)
  let b: float64 = pick(1, f)
  let c: float64 = pick(1, 2)
  let d: float64 = ident(min(1, 2))
  let _ = a
  b + c + d
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn method_call_leaves_a_constant_alone_when_the_receiver_binds_the_parameter() {
    let input = r#"
struct Wrapper<T> { value: T }

impl<T> Wrapper<T> {
  fn replace(self, v: T) -> Wrapper<T> { Wrapper { value: v } }
  fn with<U>(self, u: U) -> U { u }
}

fn test() -> float32 {
  let w: Wrapper<float64> = Wrapper { value: 1 }
  let r = w.replace(2)
  let u: float32 = w.with(3)
  let _ = r
  u
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_local_generic_return_only_type_args() {
    let input = r#"
fn make<T>() -> Slice<T> {
  Slice.new<T>()
}

fn test() -> Slice<int> {
  (make)()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_ufcs_generic_return_only_type_args() {
    let input = r#"
struct Factory {}

impl Factory {
  fn make<T>(self) -> Slice<T> {
    Slice.new<T>()
  }
}

fn test() -> Slice<int> {
  let f = Factory {}
  (Factory.make)(f)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_static_generic_return_only_type_args() {
    let input = r#"
pub struct Factory {}

impl Factory {
  pub fn make<T>() -> Slice<T> {
    Slice.new<T>()
  }
}

fn test() -> Slice<int> {
  (Factory.make)()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_go_variadic() {
    let input = r#"
import filepath "go:path/filepath"

fn test(parts: Slice<string>) -> string {
  filepath.Join(parts...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_with_leading_args_into_go_variadic() {
    let input = r#"
import filepath "go:path/filepath"

fn test(base: string, rest: Slice<string>) -> string {
  filepath.Join(base, rest...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_ufcs_method() {
    let input = r#"
struct Logger {}

impl Logger {
  fn push(self, entries: VarArgs<string>) {}
}

fn test(l: Logger, parts: Slice<string>) {
  l.push(parts...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_receiver_method_ufcs() {
    let input = r#"
struct Logger {}

impl Logger {
  pub fn push(self, entries: VarArgs<string>) {}
}

fn test(l: Logger, parts: Slice<string>) {
  Logger.push(l, parts...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_native_slice_append() {
    let input = r#"
fn test(s: Slice<int>, more: Slice<int>) -> Slice<int> {
  s.append(more...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_range_slice_into_native_append_needs_no_capacity_cap() {
    let input = r#"
fn total(xs: VarArgs<int>) -> int {
  xs.fold(0, |acc, x| acc + x)
}

fn insert_at(out: Slice<int>, pos: int, x: int) -> Slice<int> {
  let mut next: Slice<int> = []
  next = next.append(out[..pos]...)
  next = next.append(x)
  next.append(out[pos..]...)
}

fn middle(xs: Slice<int>, ys: Slice<int>) -> Slice<int> {
  xs.append(ys[1..=2]...)
}

fn sum_tail(ys: Slice<int>) -> int {
  total(ys[1..]...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_native_slice_append_assignment() {
    let input = r#"
fn test(s: Slice<int>, more: Slice<int>) -> Slice<int> {
  let mut s = s
  s = s.append(more...)
  s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_with_leading_args_into_native_slice_append_assignment() {
    let input = r#"
fn test(s: Slice<int>, extra: int, more: Slice<int>) -> Slice<int> {
  let mut s = s
  s = s.append(extra, more...)
  s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_concrete_slice_into_go_any_variadic_wraps_to_any() {
    let input = r#"
import "go:fmt"

fn main() {
  let xs = ["a", "b", "c"]
  fmt.Println(xs...)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_with_setup_preserves_sibling_eval_order() {
    let input = r#"
fn side_a() -> int { 1 }

fn get_xs() -> Result<Slice<int>, error> { Ok([1, 2, 3]) }

fn variadic(_first: int, _rest: VarArgs<int>) -> int { 0 }

fn run() -> Result<int, error> {
  Ok(variadic(side_a(), get_xs()?...))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_addressed_receiver_is_not_copied_before_effectful_arg() {
    let input = r#"
struct Foo { count: int }

impl Foo {
  fn add<T>(self: mut Ref<Foo>, _val: Bar) {
    self.count += 1
  }
}

struct Bar {}

impl Bar {
  fn new() -> Bar {
    Bar {}
  }
}

fn run() {
  let mut foo = Foo { count: 0 }
  foo.add<int>(Bar.new())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_addressed_index_receiver_pinned_before_effectful_arg() {
    let input = r#"
struct Cell { v: int }

impl Cell {
  fn add<T>(self: mut Ref<Cell>, delta: int) {
    self.v += delta
  }
}

fn bump(i: mut Ref<int>) -> int {
  i.* = 1
  7
}

fn run() {
  let mut items = [Cell { v: 0 }, Cell { v: 0 }]
  let mut i = 0
  items[i].add<int>(bump(&i))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_addressed_struct_literal_receiver_pinned_before_effectful_arg() {
    let input = r#"
struct Cell { v: int }

impl Cell {
  fn add<T>(self: mut Ref<Cell>, delta: int) {
    self.v += delta
  }
}

fn bump(i: mut Ref<int>) -> int {
  i.* = 1
  7
}

fn run() {
  let mut i = 0
  Cell { v: i }.add<int>(bump(&i))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_addressed_receiver_with_own_setup_pinned_before_effectful_arg() {
    let input = r#"
struct Cell { a: int, b: int }

impl Cell {
  fn add<T>(self: mut Ref<Cell>, delta: int) {
    self.b += delta
  }
}

fn seed() -> Result<int, error> {
  Ok(1)
}

fn bump(i: mut Ref<int>) -> Result<int, error> {
  i.* = 99
  Ok(7)
}

fn run() -> Result<(), error> {
  let mut i = 0
  Cell { a: seed()?, b: i }.add<int>(bump(&i)?)
  Ok(())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn discarded_tail_call_to_go_builtin_uses_underscore() {
    let input = r#"
fn test() {
  "test".length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn abi_transition_lowered_tail_return_partial_ok() {
    let input = r#"
fn run(x: int) -> Partial<int, error> {
  Partial.Ok(x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn abi_transition_lowered_tail_return_tuple_with_nullable_slot() {
    let input = r#"
fn run(x: int, y: Option<Ref<int>>) -> (int, Option<Ref<int>>) {
  (x, y)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn abi_transition_callback_adapter_lowered_to_tagged() {
    let input = r#"
fn double(x: int) -> Option<int> { Some(x + x) }

fn run() -> Option<int> {
  Option.and_then(Some(2), double)
}
"#;
    assert_emit_snapshot!(input);
}

// A plain function whose return type and arity coincide with a variant
// (`prepend` vs `Cons(int, IntList)`) is a normal call, not a constructor: its
// recursive arg is passed by value, never addressed.
#[test]
fn function_returning_enum_with_variant_arity_is_not_a_constructor() {
    let input = r#"
enum IntList {
  Empty,
  Cons(int, IntList),
}

fn prepend(value: int, list: IntList) -> IntList {
  IntList.Cons(value, list)
}

fn small() -> IntList {
  prepend(1, prepend(2, IntList.Empty))
}
"#;
    assert_emit_snapshot!(input);
}

// A recursive-enum constructor is a first-class value: its make-function takes
// the recursive field by value (`arg1 IntList`), so passing it to a higher-order
// function matches the expected `fn(int, IntList) -> IntList` signature.
#[test]
fn recursive_enum_constructor_passed_to_higher_order_function() {
    let input = r#"
enum IntList {
  Empty,
  Cons(int, IntList),
}

fn apply(f: fn(int, IntList) -> IntList, x: int, xs: IntList) -> IntList {
  f(x, xs)
}

fn build() -> IntList {
  apply(IntList.Cons, 1, apply(IntList.Cons, 2, IntList.Empty))
}
"#;
    assert_emit_snapshot!(input);
}

// `...T` in the signature, `[]T` in the body, as in Go.
#[test]
fn varargs_param_declares_variadic_and_bodies_use_a_slice() {
    let input = r#"
fn test(xs: VarArgs<int>) -> int {
  let mut n = xs.length() + xs[0]
  for x in xs {
    n = n + x
  };
  n
}
"#;
    assert_emit_snapshot!(input);
}

// Zero variadic args, so `T` must be forwarded explicitly: `first[int]()`.
#[test]
fn empty_varargs_call_forwards_inferred_type_arg() {
    let input = r#"
fn first<T>(xs: VarArgs<T>) -> Option<T> { None }

fn test() {
  let r: Option<int> = first()
  if r.unwrap_or(7) != 7 {
    panic("expected 7")
  }
}
"#;
    assert_emit_snapshot!(input);
}

// The fixed arg doesn't determine `T`; the variadic got none, so forward it:
// `g[int](5)`.
#[test]
fn empty_varargs_with_fixed_arg_forwards_inferred_type_arg() {
    let input = r#"
fn g<T>(head: int, rest: VarArgs<T>) -> Option<T> { None }

fn test() {
  let r: Option<int> = g(5)
  if r.unwrap_or(9) != 9 {
    panic("expected 9")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn empty_varargs_method_call_forwards_inferred_type_arg() {
    let input = r#"
struct Box {}

impl Box {
  fn first<T>(self, xs: VarArgs<T>) -> Option<T> { None }
}

fn test() {
  let b = Box {}
  let r: Option<int> = b.first()
  if r.unwrap_or(7) != 7 {
    panic("expected 7")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pub_function_same_package_call_uses_exported_name() {
    let input = r#"
import "go:fmt"

pub fn greet_all() -> string {
  "hi"
}

fn main() {
  fmt.Println(greet_all())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_param_shadows_package_struct() {
    let input = r#"
struct T {
  v: int,
}

fn make_t() -> T {
  T { v: 7 }
}

fn f<T>(x: T) -> int {
  let mut opt = None
  opt = Some(make_t())
  match opt {
    Some(t) => t.v,
    None => 0,
  }
}

fn test() -> int {
  f("hi")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_param_shadows_enum_variant_constant_and_constructor() {
    let input = r#"
enum Wrap {
  Empty,
  Full(int),
  Partial(int),
}

fn describe<WrapFull>(w: Wrap, _tag: WrapFull) -> int {
  match w {
    Wrap.Empty => 0,
    Wrap.Full(n) => n,
    Wrap.Partial(m) => m + 100,
  }
}

fn build<MakeWrapFull>(_seed: MakeWrapFull) -> int {
  match Wrap.Full(7) {
    Wrap.Full(n) => n,
    _ => 0,
  }
}

fn test() -> int {
  describe(Wrap.Full(42), "x") + build(true)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_param_shadows_import_qualifier() {
    let input = r#"
import sc "go:strconv"

fn label<sc>(_tag: sc) -> string {
  sc.Itoa(5)
}

fn test() -> string {
  label(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn renamed_generic_freshens_colliding_value_param() {
    let input = r#"
import sc "go:strconv"

fn label<sc>(sc_2: int, _tag: sc) -> string {
  sc.Itoa(sc_2)
}

fn test() -> string {
  label(5, 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn renamed_generic_freshens_colliding_local() {
    let input = r#"
import sc "go:strconv"

fn label<sc>(_tag: sc) -> string {
  let sc_2 = 9
  sc.Itoa(sc_2)
}

fn test() -> string {
  label(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn renamed_generic_type_in_closure_after_colliding_local() {
    let input = r#"
import sc "go:strconv"

fn make<sc>(x: sc) -> fn() -> sc {
  || -> sc {
    let sc_2 = 1
    let _ = sc.Itoa(sc_2)
    let mut opt = None
    opt = Some(x)
    match opt {
      Some(v) => v,
      None => x,
    }
  }
}

fn test() -> int {
  let f = make(7)
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn self_type_in_every_impl_position() {
    let input = r#"
struct Pair<T> {
  first: T,
  second: T,
}

impl<T> Pair<T> {
  fn head(self) -> T {
    self.first
  }

  fn peek(self: Ref<Self>) -> T {
    self.second
  }

  fn swap(self: mut Ref<Self>) {
    let held = self.first
    self.first = self.second
    self.second = held
  }

  fn twin(self, other: Self) -> Self {
    other
  }

  fn boxed(self) -> Option<Self> {
    Some(self)
  }

  fn local(self) -> T {
    let copy: Self = self
    copy.first
  }
}

fn run() {
  let mut p = Pair { first: 1, second: 2 }
  p.swap()
  let _ = p.head()
  let _ = p.peek()
  let _ = p.twin(p)
  let _ = p.boxed()
  let _ = p.local()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn self_type_does_not_capture_method_generics() {
    let input = r#"
struct Box<T> {
  value: T,
}

impl<T> Box<T> {
  fn map<U>(self, f: fn(T) -> U) -> Box<U> {
    Box { value: f(self.value) }
  }

  fn same(self) -> Self {
    self
  }
}

fn run() {
  let b = Box { value: 1 }
  let _ = b.same()
  let _ = b.map(|n| n == 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn self_type_matches_the_spelled_out_receiver() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn spelled(self: mut Ref<Counter>) {
    self.value += 1
  }

  fn shorthand(self: mut Ref<Self>) {
    self.value += 1
  }
}

fn run() {
  let mut c = Counter { value: 0 }
  c.spelled()
  c.shorthand()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_setup_and_nested_let_preserve_evaluation_order() {
    let input = r#"
fn mark(trace: mut Ref<int>, value: int) -> int {
  trace.* = trace.* * 100 + value
  value
}

fn choose(trace: mut Ref<int>, flag: bool) -> int {
  return {
    let result = if flag { mark(trace, 10) } else { mark(trace, 20) }
    result + mark(trace, 1)
  }
}

fn main() {
  let mut trace = 0
  let first = choose(&trace, true)
  if first != 11 || trace != 1001 { panic("then return evaluated out of order") }
  let second = choose(&trace, false)
  if second != 21 || trace != 10012001 { panic("else return evaluated out of order") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_lisette_function_destructures_the_multi_return() {
    let input = r#"
import "go:fmt"

fn pair(flag: bool) -> (string, bool) {
  if flag { ("yes", true) } else { ("no", false) }
}

fn main() {
  let (word, ok) = pair(true)
  fmt.Println(word, ok)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_call_in_return_position_returns_the_call() {
    let input = r#"
interface Named {
  fn name() -> string
}

struct Dog {}

impl Dog {
  fn name(self) -> string { "dog" }
}

struct Lowering { offset: int }

impl Lowering {
  fn pair(self, x: int) -> (int, string) { (x + self.offset, "p") }

  fn statement(self, x: int) -> (int, string) {
    if x > 10 { return self.pair(x) }
    if x > 5 {
      self.pair(x + 1)
    } else {
      (0, "none")
    }
  }
}

fn dup<T>(value: T) -> (T, T) { (value, value) }

fn twice(n: int) -> (int, int) {
  dup(n)
}

fn dog_pair() -> (Dog, int) { (Dog {}, 1) }

fn named_pair() -> (Named, int) {
  dog_pair()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_generic_function_destructures_the_multi_return() {
    let input = r#"
import "go:fmt"

fn first_and_len<T>(xs: Slice<T>) -> (Option<T>, int) {
  (xs.get(0), xs.length())
}

fn main() {
  let (head, n) = first_and_len([1, 2, 3])
  fmt.Println(head, n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_method_destructures_the_multi_return() {
    let input = r#"
import "go:fmt"

struct Store { count: int }

impl Store {
  fn split(self) -> (int, int) {
    (self.count / 2, self.count - self.count / 2)
  }
}

fn main() {
  let store = Store { count: 7 }
  let (left, right) = store.split()
  fmt.Println(left, right)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_ufcs_call_destructures_the_multi_return() {
    let input = r#"
import "go:fmt"

struct Interval { lo: int, hi: int }

impl Interval {
  fn bounds(self) -> (int, int) {
    (self.lo, self.hi)
  }
}

fn main() {
  let r = Interval { lo: 1, hi: 9 }
  let (low, high) = Interval.bounds(r)
  fmt.Println(low, high)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_interface_method_destructures_the_multi_return() {
    let input = r#"
import "go:fmt"

interface Splitter {
  fn split() -> (int, int)
}

struct Even { n: int }

impl Even {
  fn split(self) -> (int, int) {
    (self.n / 2, self.n / 2)
  }
}

fn total(s: Splitter) -> int {
  let (a, b) = s.split()
  a + b
}

fn main() {
  fmt.Println(total(Even { n: 8 }))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_with_wildcard_element_from_lisette_function() {
    let input = r#"
import "go:fmt"

fn pair(flag: bool) -> (string, bool) {
  if flag { ("yes", true) } else { ("no", false) }
}

fn main() {
  let (_, only_flag) = pair(false)
  fmt.Println(only_flag)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_with_unused_elements_from_lisette_function_discards() {
    let input = r#"
import "go:fmt"

fn pair(flag: bool) -> (string, bool) {
  fmt.Println("called")
  if flag { ("yes", true) } else { ("no", false) }
}

fn main() {
  let (_, _) = pair(false)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_lisette_function_avoids_package_names() {
    let input = r#"
import "go:fmt"

fn bounds() -> (int, int) {
  (1, 10)
}

fn low() -> int { 0 }

fn main() {
  let (low, high) = bounds()
  fmt.Println(low, high)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_closure_value_reads_the_tuple_value() {
    let input = r#"
import "go:fmt"

fn main() {
  let pair = |n: int| (n, n + 1)
  let (a, b) = pair(3)
  fmt.Println(a, b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_let_from_lisette_function_shadowing_an_outer_binding() {
    let input = r#"
import "go:fmt"

fn pair(n: int) -> (int, int) {
  (n, n * 2)
}

fn main() {
  let a = 5
  let (a, b) = pair(a)
  fmt.Println(a, b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_local_read_by_index_destructures_the_multi_return() {
    let input = r#"
import "go:fmt"

fn pair(n: int) -> (int, int) {
  (n, n + 1)
}

fn main() {
  let p = pair(3)
  fmt.Println(p.0 + p.1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_local_with_one_index_read_discards_the_rest() {
    let input = r#"
import "go:fmt"

fn trio(n: int) -> (int, string, bool) {
  (n, "a", true)
}

fn main() {
  let t = trio(3)
  fmt.Println(t.1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_local_read_both_by_index_and_whole_keeps_every_element() {
    let input = r#"
import "go:fmt"

fn pair(n: int) -> (int, int) {
  (n, n + 1)
}

fn total(p: (int, int)) -> int {
  p.0 + p.1
}

fn main() {
  let p = pair(3)
  fmt.Println(p.0 + total(p))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_local_read_by_index_avoids_a_later_binding_name() {
    let input = r#"
import "go:fmt"

fn pair(n: int) -> (int, int) {
  (n, n + 1)
}

fn main() {
  let p = pair(3)
  let p0 = 99
  fmt.Println(p.0 + p0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_local_from_a_nested_tuple_reads_the_inner_element() {
    let input = r#"
import "go:fmt"

fn nested(n: int) -> ((int, int), int) {
  ((n, n), n)
}

fn main() {
  let p = nested(3)
  fmt.Println(p.0.0 + p.1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn mutable_tuple_local_keeps_the_whole_value() {
    let input = r#"
import "go:fmt"

fn pair(n: int) -> (int, int) {
  (n, n + 1)
}

fn main() {
  let mut p = pair(3)
  p = pair(4)
  fmt.Println(p.0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pointer_receiver_method_with_call_arguments_is_not_pinned() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn add(self: mut Ref<Counter>, k: int) {
    self.n += k
  }
}

fn main() {
  let mut c = Counter { n: 1 }
  let bump = || -> int { c.n += 10; 1 }
  c.add(bump())
  let _ = c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn value_receiver_method_with_mutating_arguments_keeps_the_pin() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn plus(self, k: int) -> int {
    self.n + k
  }
}

fn main() {
  let mut c = Counter { n: 1 }
  let bump = || -> int { c.n += 10; 1 }
  let v = c.plus(bump())
  let _ = v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pointer_receiver_method_on_a_pointer_local_keeps_the_pin() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn add(self: mut Ref<Counter>, k: int) {
    self.n += k
  }
}

fn main() {
  let mut a = Counter { n: 1 }
  let mut b = Counter { n: 2 }
  let mut p = &a
  let swap = || -> int { p = &b; 1 }
  p.add(swap())
  let _ = (a, b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pointer_receiver_method_on_an_indexed_element_keeps_the_pin() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn add(self: mut Ref<Counter>, k: int) {
    self.n += k
  }
}

fn main() {
  let mut cs = [Counter { n: 1 }, Counter { n: 2 }]
  let mut i = 0
  let bump = || -> int { i = 1; 1 }
  cs[i].add(bump())
  let _ = cs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_pointer_receiver_method_with_call_arguments_is_not_pinned() {
    let input = r#"
import "go:bytes"

fn main() {
  let mut buf = bytes.Buffer {}
  let mut n = 0
  let next = || -> int { n += 1; n }
  buf.Grow(next())
  let _ = buf.Len()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pointer_receiver_variadic_method_with_call_arguments_is_not_pinned() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn add_all(self: mut Ref<Counter>, ks: VarArgs<int>) {
    for k in ks { self.n += k }
  }
}

fn main() {
  let mut c = Counter { n: 1 }
  let bump = || -> int { c.n += 10; 1 }
  c.add_all(bump(), bump())
  c.add_all(bump())
  let _ = c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn an_operand_read_after_its_own_setup_stays_before_a_later_call() {
    let input = r#"
fn add(a: int, b: int) -> int {
  a * 100 + b
}

fn parse(n: int) -> Result<int, string> {
  if n > 0 { Ok(n) } else { Err("negative") }
}

fn sum_before_bump(n: int) -> Result<int, string> {
  let mut x = 1
  let bump = || {
    x = 10
    0
  }
  Ok(add(parse(n)? + x, bump()))
}

fn test() {
  match sum_before_bump(1) {
    Ok(value) => { if value != 200 { panic(f"the operand read a later write: {value}") } },
    Err(_) => panic("parsing failed"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_functions_fill_a_generic_result() {
    let input = r#"
struct Holder<U> {
  f: fn(int) -> U,
}

fn apply<T, U>(value: T, f: fn(T) -> U) -> U {
  f(value)
}

fn record(total: mut Ref<int>, n: int) {
  total.* += n
}

fn test() {
  let mut calls = 0
  apply(1, |n: int| -> () { calls += n })
  let _ = Some(2).map(|n: int| { calls += n; () })
  let mut total = 0
  let add = |n: int| record(&total, n)
  apply(3, add)
  let holder = Holder { f: add }
  holder.f(4)
  if calls != 3 || total != 7 { panic(f"calls={calls} total={total}") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tagged_method_value_in_lowered_fn_slot_calls_method_once() {
    let input = r#"
interface Getter<T> {
  fn get() -> T
}

struct Box {
  n: int,
}

impl Box {
  fn get(self) -> Result<int, error> {
    Ok(self.n)
  }
}

fn use_getter<G: Getter<Result<int, error>>>(g: G) -> Result<int, error> {
  g.get()
}

fn take(f: fn() -> Result<int, error>) -> Result<int, error> {
  f()
}

fn main() {
  let b = Box { n: 1 }
  let _ = use_getter(b)
  let g = b.get
  let _ = take(g)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generated_locals_avoid_import_qualifiers() {
    let input = r#"
import "go:fmt"
import i "go:strings"
import result "go:strconv"

fn qualified_call(s: string) {
  for b in s.bytes() {
    fmt.Println(i.ToUpper("x"), b)
  }
}

fn qualified_type(s: string) {
  for b in s.bytes() {
    let xs: Slice<i.Builder> = []
    fmt.Println(xs, b)
  }
}

fn mapped(xs: Slice<int>) {
  fmt.Println(xs.map(|x| x + 1), result.Itoa(1))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn lambda_body_gets_the_same_cleanup_as_a_function_body() {
    let input = r#"
import "go:fmt"

fn pick(xs: Slice<int>, i: int) -> int {
  match xs.get(i) { Some(value) => value, None => -1 }
}

fn main() {
  let f = |xs: Slice<int>, i: int| -> int {
    match xs.get(i) { Some(value) => value, None => -1 }
  }
  fmt.Println(f([7], 0), pick([7], 0))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn lambda_locals_ignore_the_enclosing_assign_target() {
    let input = r#"
import "go:fmt"

fn pick(n: int) -> int {
  let total = if n > 0 {
    let f = |k: int| -> int { let total = k * 2; fmt.Println(total); total + 1 }
    f(n)
  } else { 0 }
  fmt.Println(total)
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generated_identity_fused_capture() {
    let input = r#"
fn main() {
  let found = 40
  let ret = 2
  let f = || {
    let found = [ret, found].find(|found| found > 3)
    let Some(ret) = found else { panic("find lost payload") }
    let read = || ret
    read()
  }
  let mut next = Some(3)
  let mut total = 0
  while let Some(found_1) = next {
    let read = || found_1 + f()
    total += read()
    next = None
  }
  if total != 43 || found != 40 || ret != 2 { panic("capture changed") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generated_identity_tuple_callback() {
    let input = r#"
import "go:encoding/pem"

fn apply(f: fn(Slice<byte>) -> (Option<Ref<pem.Block>>, Slice<byte>), data: Slice<byte>) -> int {
  let (ret, ret_1) = f(data)
  if ret.is_some() { panic("unexpected PEM") }
  let read = || ret_1.length()
  read()
}

fn main() {
  let cb = 11
  let f = || {
    let cb = apply(pem.Decode, [65, 66])
    let ch = Channel.buffered<int>(1)
    ch.send(cb)
    select {
      let Some(ret) = ch.receive() => { let read = || ret; read() },
      _ => 0,
    }
  }
  if f() != 2 || cb != 11 { panic("tuple callback capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generated_identity_wrapped_arm() {
    let input = r#"
import "go:strconv"

fn n() -> string { "context" }

fn main() {
  let err = 9
  let f = || {
    match strconv.Atoi("7").wrap_err(n()) {
      Ok(n) => { let read = || n + err; read() },
      Err(_) => 0,
    }
  }
  let mut total = 0
  for ret in [1, 2] {
    if let Some(err) = Some(ret) {
      let read = || err + f()
      total += read()
    }
  }
  if total != 35 || err != 9 { panic("wrapped arm capture") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_identity_or_pattern_capture() {
    let input = r#"
enum Choice { A(int), B(int), Missing }

fn choose(item: Choice) -> int {
  let value = 40
  let read = || value
  let Choice.A(value) | Choice.B(value) = item else { return read() + 2 }
  let f = || {
    let Choice.A(value) | Choice.B(value) = Choice.B(value + 1) else { return read() }
    value + read()
  }
  f()
}

fn main() {
  if choose(Choice.B(7)) != 48 || choose(Choice.Missing) != 42 { panic("or-pattern identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_identity_mapped_default_capture() {
    let input = r#"
fn main() {
  let target = 7
  let f = || {
    let target = [1].get(0).map(|ret| ret + target).unwrap_or(0)
    let read = || target
    read()
  }
  let mut total = 0
  for target in [2, 3] {
    total += match target {
      value if value > 0 => { let read = || value; read() + f() },
      _ => 0,
    }
  }
  if total != 21 || target != 7 { panic("mapped default identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_identity_disjoint_generated_spelling() {
    let input = r#"
fn main() {
  let value = 7
  let f = || { let value = value + 1; value }
  let mut next = Some(3)
  let mut total = 0
  while let Some(value_1) = next {
    let g = || { let value_1 = value_1 + 1; value_1 + f() }
    total += g()
    next = None
  }
  let ch = Channel.buffered<int>(1)
  ch.send(5)
  total += select {
    let Some(value_1) = ch.receive() => { let g = || value_1 + f(); g() },
    _ => 0,
  }
  if total != 25 || value != 7 { panic("disjoint local identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_identity_const_capture() {
    let input = r#"
fn main() {
  const MAX_SIZE = 7
  let f = || {
    const INNER_SIZE = 3
    match INNER_SIZE { 3 => INNER_SIZE, _ => 0 }
  }
  if f() != 3 { panic("constant capture") }
  let value = match MAX_SIZE { 7 => MAX_SIZE, _ => 0 }
  if value != 7 { panic("constant subject") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tagged_argument_collections() {
    let input = r#"
import "go:mime/multipart"

fn file(n: int) -> Option<mut Ref<multipart.FileHeader>> {
  let mut header = multipart.FileHeader { Filename: "file", Header: None, Size: n as int64 }
  Some(&header)
}

fn main() {
  let picked = Option.and_then(Some(3), file)
  let mut files = Map.new<string, mut Slice<Option<mut Ref<multipart.FileHeader>>>>()
  files["key"] = [picked, None]
  let form = multipart.Form { Value: None, File: Some(files) }
  let Some(fields) = form.File else { panic("missing files") }
  let Some(roundtrip) = fields.get("key") else { panic("missing slice") }
  if roundtrip.length() != 2 || roundtrip[1].is_some() { panic("collection shape") }
  let Some(header) = roundtrip[0] else { panic("missing header") }
  if header.Size != 3 { panic("collection payload") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tagged_argument_channel_variadic() {
    let input = r#"
import "go:fmt"
import "go:time"

fn channel(n: int) -> Option<Receiver<time.Time>> {
  time.Tick(n as time.Duration)
}

fn label(n: int) -> Option<string> { Some(fmt.Sprint(n)) }

fn main() {
  if Option.and_then(Some(0), channel).is_some() { panic("nil channel") }
  let value = Option.and_then(Some(7), label)
  let absent: Option<string> = None
  let rendered = fmt.Sprint(value, absent)
  if value.unwrap_or("bad") != "7" || rendered.is_empty() { panic("variadic options") }
  let ch = Channel.buffered<Option<string>>(2)
  ch.send(value)
  ch.send(absent)
  if ch.receive().unwrap_or(None).unwrap_or("bad") != "7" || ch.receive().unwrap_or(None).is_some() { panic("channel options") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tagged_argument_go_function_type() {
    let input = r#"
import "go:bufio"
import "go:strings"

fn split(data: mut Slice<byte>, at_eof: bool) -> Result<(int, Slice<byte>), error> {
  bufio.ScanWords(data, at_eof)
}

fn count(n: int) -> Option<int> {
  let splitter: bufio.SplitFunc = split
  let mut scanner = bufio.NewScanner(strings.NewReader("one two"))
  scanner.Split(splitter)
  let mut total = n
  while scanner.Scan() { total += 1 }
  Some(total)
}

fn main() {
  if Option.and_then(Some(3), count).unwrap_or(-1) != 5 { panic("Go function type") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_setup_struct_update_capture() {
    let input = r#"
struct Item { value: int, marker: int }
fn main() {
  let base = Item { value: 7, marker: 3 }
  let out = base
  let read = || out.value
  let f = || {
    let out = Item { value: read() + 1, ..out }
    let read = || out.value + out.marker
    read()
  }
  let mut total = 0
  for out in [1, 2] { total += f() + out }
  if total != 25 || read() != 7 { panic("updated temp identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_setup_field_subject_capture() {
    let input = r#"
struct Item { value: Option<int> }
fn main() {
  let subject = 7
  let f = || {
    let Some(subject) = (Item { value: Some(subject + 1) }).value else { return subject }
    let g = || subject
    g()
  }
  let mut pending = Some(2)
  let mut total = 0
  while let Some(subject) = pending {
    total += subject + f()
    pending = None
  }
  let ch = Channel.buffered<int>(1)
  ch.send(3)
  total += select {
    let Some(subject) = ch.receive() => subject + f(),
    _ => 0,
  }
  if total != 21 || subject != 7 { panic("subject temp identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_setup_branch_target_capture() {
    let input = r#"
fn main() {
  let result = 7
  let f = || {
    let result = if result > 0 { 3 } else { 4 }
    let read = || result
    let flag = if read() == 3 { true } else { false }
    if flag { read() } else { 0 }
  }
  let mut total = 0
  for result in [Some(2), None] {
    if let Some(result) = result { total += result + f() }
    else { total += f() }
  }
  if total != 8 || result != 7 { panic("branch target identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_tagged_callback_unit_result() {
    let input = r#"
fn okay<T>(x: T) -> Result<T, error> { Ok(x) }
fn main() {
  let result = Option.map(Some(()), okay)
  let Some(Ok(_)) = result else { panic("generic unit result") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_tagged_callback_tuple_option() {
    let input = r#"
fn pair<T>(x: T) -> (T, T) { (x, x) }
fn main() {
  let x = 3
  let result = Option.map(Some(Some(&x)), pair)
  let Some((Some(left), Some(right))) = result else { panic("generic tuple options") }
  if left != &x || right != &x { panic("generic tuple payload") }
  let (direct_left, direct_right) = pair(Some(&x))
  if direct_left.is_none() || direct_right.is_none() { panic("generic direct tuple") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_tagged_callback_option_reference() {
    let input = r#"
fn some<T>(x: T) -> Option<T> { Some(x) }
fn main() {
  let x = 3
  let got = Option.and_then(Some(&x), some)
  let Some(value) = got else { panic("generic option reference") }
  if value != &x { panic("generic reference payload") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_local_zero_collections_capture() {
    let input = r#"
fn main() {
  let s = 3
  let i = 7
  let arr = [9]
  let f = || {
    let s = Slice.make<Option<int>>(s)
    let arr = Array.new<Option<int>, 2>()
    let mut count = 0
    for i in s { if i.is_none() { count += 1 } }
    for i in arr { if i.is_none() { count += 1 } }
    count
  }
  let Some(converted) = Array.from<int, 1>(arr) else { panic("array conversion") }
  if f() != 5 || converted[0] != 9 || i != 7 { panic("helper local captures") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_local_type_assert_capture() {
    let input = r#"
import "go:fmt"
import "go:time"
fn inspect(pair: (fmt.Stringer, int)) -> int {
  let ok = 7
  let f = || {
    match pair {
      (time.Duration(value), ok) => value as int + ok,
      _ => ok,
    }
  }
  f() + ok
}
fn main() {
  let value: fmt.Stringer = 3 as time.Duration
  if inspect((value, 2)) != 12 { panic("type assertion identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_local_callback_parameter_capture() {
    let input = r#"
fn next(arg0: int) -> Option<int> { Some(arg0 + 1) }
fn main() {
  let arg0 = 7
  let cb = || {
    let arg0 = Option.and_then(Some(arg0), next).unwrap_or(0)
    let read = || arg0
    read()
  }
  let mut pending = Some(2)
  let mut total = 0
  while let Some(arg0) = pending {
    total += arg0 + cb()
    pending = None
  }
  let ch = Channel.buffered<int>(1)
  ch.send(3)
  total += select {
    let Some(arg0) = ch.receive() => arg0 + cb(),
    _ => 0,
  }
  if total != 21 || arg0 != 7 { panic("callback parameter identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_value_option_reference() {
    let input = r#"
fn some<T>(x: T) -> Option<T> { Some(x) }
fn consume(f: fn(Ref<int>) -> Option<Ref<int>>, x: Ref<int>) -> Option<Ref<int>> { f(x) }
fn main() {
  let x = 3
  let stored: fn(Ref<int>) -> Option<Ref<int>> = some
  let Some(first) = stored(&x) else { panic("stored generic option") }
  let Some(second) = consume(some, &x) else { panic("generic option callback") }
  if first != &x || second != &x { panic("generic option payload") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_value_tuple() {
    let input = r#"
fn pair<T>(x: T) -> (T, T) { (x, x) }
fn main() {
  let x = 3
  let stored: fn(Option<Ref<int>>) -> (Option<Ref<int>>, Option<Ref<int>>) = pair
  let (Some(first), Some(second)) = stored(Some(&x)) else { panic("stored generic tuple") }
  let inferred = pair
  let Some((Some(third), Some(fourth))) = Option.map(Some(Some(&x)), inferred) else { panic("inferred generic tuple") }
  if first != &x || second != &x || third != &x || fourth != &x { panic("generic tuple payloads") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_value_tuple_alias() {
    let input = r#"
type Pair<T> = (T, T)
fn pair<T>(x: T) -> Pair<T> { (x, x) }
fn main() {
  let x = 3
  let (Some(first), Some(second)) = pair(Some(&x)) else { panic("generic tuple alias") }
  let Some((Some(third), Some(fourth))) = Option.map(Some(Some(&x)), pair) else { panic("generic alias callback") }
  let stored = pair
  let (Some(fifth), Some(sixth)) = stored(Some(&x)) else { panic("generic stored alias") }
  if first != &x || second != &x || third != &x || fourth != &x || fifth != &x || sixth != &x { panic("generic alias payloads") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_value_unit_result() {
    let input = r#"
fn okay<T>(x: T) -> Result<T, error> { Ok(x) }
fn consume(f: fn(()) -> Result<(), error>) -> bool { f(()).is_ok() }
fn main() {
  if !consume(okay) { panic("generic callback unit result") }
  let stored: fn(()) -> Result<(), error> = okay
  if stored(()).is_err() { panic("stored generic unit result") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unused_tuple_subject_branch() {
    let input = r#"
fn pair(ch: Channel<int>) -> (int, int) { ch.send(7); (1, 2) }
fn choose(ch: Channel<int>, flag: bool) {
  let (_, _) = (if flag { pair(ch) } else { pair(ch) }) else { panic("irrefutable branch tuple") }
}
fn main() {
  let ch = Channel.buffered<int>(3)
  choose(ch, true)
  ch.send(9)
  if ch.receive().unwrap_or(0) != 7 { panic("branch subject omitted") }
  if ch.receive().unwrap_or(0) != 9 { panic("branch subject repeated") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unused_tuple_subject_named() {
    let input = r#"
fn pair(ch: Channel<int>) -> (int, int) { ch.send(7); (1, 2) }
fn main() {
  let ch = Channel.buffered<int>(3)
  let first = 7
  let read = || first
  let f = || {
    let (first, second) = pair(ch) else { panic("irrefutable named tuple") }
    read()
  }
  if f() != 7 { panic("unused binding replaced capture") }
  ch.send(9)
  if ch.receive().unwrap_or(0) != 7 { panic("named subject omitted") }
  if ch.receive().unwrap_or(0) != 9 { panic("named subject repeated") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unused_tuple_subject_nested_unit() {
    let input = r#"
fn pair(ch: Channel<int>) -> ((), (int, ())) { ch.send(7); ((), (2, ())) }
fn main() {
  let ch = Channel.buffered<int>(3)
  let (_, (_, _)) = pair(ch) else { panic("irrefutable nested tuple") }
  ch.send(9)
  if ch.receive().unwrap_or(0) != 7 { panic("nested subject omitted") }
  if ch.receive().unwrap_or(0) != 9 { panic("nested subject repeated") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unused_tuple_subject_wildcards() {
    let input = r#"
fn pair(ch: Channel<int>) -> (int, int) { ch.send(7); (1, 2) }
fn main() {
  let ch = Channel.buffered<int>(3)
  let (_, _) = pair(ch) else { panic("irrefutable wildcard tuple") }
  ch.send(9)
  if ch.receive().unwrap_or(0) != 7 { panic("tuple subject omitted") }
  if ch.receive().unwrap_or(0) != 9 { panic("tuple subject repeated") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_header_identity_or_capture() {
    let input = r#"
enum Item { A(int), B(int), Empty }
fn value(ch: Channel<int>, x: int) -> Item { ch.send(x); Item.A(x) }
fn main() {
  let subject = 7
  let ch = Channel.buffered<int>(3)
  let f = || match value(ch, subject) {
    Item.A(subject) | Item.B(subject) => {
      let read = || subject
      let Some(subject) = Some(read() + 1) else { return read() }
      if let Some(subject) = Some(subject + 1) { subject } else { read() }
    },
    Item.Empty => subject,
  }
  if f() != 9 || subject != 7 { panic("or pattern capture") }
  ch.send(9)
  if ch.receive().unwrap_or(0) != 7 || ch.receive().unwrap_or(0) != 9 { panic("or pattern evaluation") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_header_identity_type_select() {
    let input = r#"
import "go:fmt"
import "go:time"
fn value(ch: Channel<int>) -> fmt.Stringer { ch.send(7); 3 as time.Duration }
fn main() {
  let subject = 7
  let effects = Channel.buffered<int>(3)
  let ch = Channel.buffered<int>(1)
  ch.send(2)
  let f = || match value(effects) {
    time.Duration(subject) => {
      let read = || subject as int
      select { let Some(subject) = ch.receive() => subject + read(), _ => 0 }
    },
    _ => subject,
  }
  if f() != 5 || subject != 7 { panic("type switch capture") }
  effects.send(9)
  if effects.receive().unwrap_or(0) != 7 || effects.receive().unwrap_or(0) != 9 { panic("type switch evaluation") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_header_identity_while_capture() {
    let input = r#"
fn value(ch: Channel<int>, x: int) -> int { ch.send(x); x }
fn main() {
  let subject = 7
  let ch = Channel.buffered<int>(3)
  let read = || subject
  let f = || match value(ch, subject + 1) {
    8 => { let subject = read() + 1; let g = || subject; g() },
    _ => 0,
  }
  let mut pending = Some(2)
  let mut total = 0
  while let Some(subject) = pending { total += subject + f(); pending = None }
  ch.send(9)
  if total != 10 || subject != 7 { panic("match header capture") }
  if ch.receive().unwrap_or(0) != 8 || ch.receive().unwrap_or(0) != 9 { panic("match header evaluation") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_type_parameter_slice() {
    let input = r#"
fn count<s>(_seed: s) -> int { Slice.make<Option<s>>(2).length() }
fn count_index<i>(_seed: i) -> int { Slice.make<Option<i>>(3).length() }
fn main() { if count(1) != 2 || count_index(1) != 3 { panic("generic slice helper") } }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_type_parameter_array() {
    let input = r#"
fn count<arr>(_seed: arr) -> int {
  let values = Array.new<Option<arr>, 2>()
  let mut count = 0
  for value in values { if value.is_none() { count += 1 } }
  count
}
fn main() { if count(1) != 2 { panic("generic array helper") } }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_type_parameter_callback() {
    let input = r#"
fn some<T>(value: T) -> Option<T> { Some(value) }
fn adapt<arg0>(seed: arg0) -> Option<arg0> { Option.and_then(Some(seed), some) }
fn main() { if adapt(7).unwrap_or(0) != 7 { panic("generic callback helper") } }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn helper_package_type_namespace() {
    let input = r#"
struct s { value: int }
fn main() {
  let values = Slice.make<Option<s>>(2)
  for value in values { if value.is_some() { panic("zero") } }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_void_interface_u_direct_generic() {
    let input = r#"
type U = ()
interface Action { fn Run() -> U }
struct Counter { n: mut Ref<int> }
impl Counter { fn Run(self: mut Ref<Counter>) -> U { self.n.* += 1; () } }
fn apply<T>(f: fn() -> T) -> T { f() }
fn main() {
 let mut n = 0
 let mut c = Counter { n: &n }
 let action = &c as Action
 let _ = apply(action.Run)
 if n != 1 { panic("direct generic interface method count") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn scalar_fn_option_reference() {
    let input = r#"
struct Holder<T, U> { f: fn(T) -> U }
fn some<T>(x: T) -> Option<T> { Some(x) }
fn main() {
  let x = 3
  let mut holder: Holder<Ref<int>, Option<Ref<int>>> = Holder { f: some }
  let Some(a) = holder.f(&x) else { panic("field call") }
  let stored = holder.f
  let Some(b) = stored(&x) else { panic("stored field") }
  holder.f = some
  let Some(c) = holder.f(&x) else { panic("assigned field") }
  if a != &x || b != &x || c != &x { panic("field payloads") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn signature_generic_factory_capture_option() {
    let input = r#"
fn check(ok:bool){if !ok {panic("assertion failed")}}
fn make<T>(value:T)->fn()->T { || value }
fn main(){let f=make(Some(5));let v=f();check(v.unwrap_or(0)==5)}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn higher_callback_variadic_tuple() {
    let input = r#"
fn first<T>(fs: VarArgs<fn(fn(int) -> T) -> T>) -> fn(fn(int) -> T) -> T { fs[0] }
fn invoke(f: fn(int) -> (int, int)) -> (int, int) { f(4) }
fn main() {
 let f = first<(int, int)>([invoke]...)
 let (a,b) = f(|x| (x,x+1))
 if a != 4 || b != 5 { panic("tuple") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_value_with_alias_parameter_fills_spelled_out_slot() {
    let input = r#"
type PairFn = fn(int) -> (int, int)

fn invoke(f: PairFn) -> (int, int) { f(4) }

fn first(f: PairFn) -> int { f(4).0 }

fn main() {
  let gs: Slice<fn(fn(int) -> (int, int)) -> (int, int)> = [invoke]
  let (a, b) = gs[0](|x| (x, x + 1))
  if a != 4 || b != 5 { panic("tuple") }
  let hs: Slice<fn(fn(int) -> (int, int)) -> int> = [first]
  if hs[0](|x| (x, x)) != 4 { panic("first") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn higher_callback_go_params_callback_commaok() {
    let input = r#"
import "go:sort"
fn give<T>(f:fn(int,fn(int)->int)->T)->fn(int,fn(int)->int)->T{f}
fn main(){let mut calls=0;let f=give(sort.Find);let Some(n)=f(5,|i|{calls+=1;2-i}) else {panic("none")};if n!=2 || calls<1{panic("search")};if f(5,|_|1).is_some(){panic("missing")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn higher_callback_go_params_callback_named_error() {
    let input = r#"
import "go:path/filepath"
fn give<T>(f:fn(string,filepath.WalkFunc)->T)->fn(string,filepath.WalkFunc)->T{f}
fn main(){let mut calls=0;let f=give(filepath.Walk);let cb:filepath.WalkFunc=|_,_,_|{calls+=1;Ok(())};if f("/__lisette_nonexistent_callback_review__",cb).is_err(){panic("walk")};if calls!=1{panic("calls")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn higher_callback_go_params_nullable_cookiejar() {
    let input = r#"
import "go:net/http/cookiejar"
fn give<T>(f:fn(Option<Ref<cookiejar.Options>>)->T)->fn(Option<Ref<cookiejar.Options>>)->T{f}
fn main(){let f=give(cookiejar.New);if f(None).is_err(){panic("none")};let opts=cookiejar.Options{PublicSuffixList:None};if f(Some(&opts)).is_err(){panic("some")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn higher_callback_go_params_optional_fn_result() {
    let input = r#"
import "go:reflect"
fn give<T>(f:fn(Unknown)->T)->fn(Unknown)->T{f}
fn main(){let mut xs=[1,2];let f=give(reflect.Swapper);let Some(swap)=f(xs) else {panic("none")};swap(0,1);if xs[0]!=2 || xs[1]!=1{panic("swap")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn container_go_split_containers_spaced() {
    let input = r#"
import "go:bufio"
struct Pair(bufio.SplitFunc,int)
fn main(){let s:Slice<bufio.SplitFunc> =[bufio.ScanWords];let a:Array<bufio.SplitFunc,1> =[bufio.ScanWords];let m:Map<string,bufio.SplitFunc> =Map.from([("f",bufio.ScanWords)]);let t:(bufio.SplitFunc,int)=(bufio.ScanWords,1);let p=Pair(bufio.ScanWords,1);let Some(mf)=m.get("f") else {panic("map")};for f in [s[0],a[0],mf,t.0,p.0]{let Ok((n,token))=f([65,32],true) else {panic("split")};if n!=2||token.length()!=1||token[0]!=65{panic("split result")}}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pattern_callback_or() {
    let input = r#"
enum E<T>{A(fn()->T),B(fn()->T)}
fn make<T>(f:fn()->T,first:bool)->E<T>{if first{E.A(f)}else{E.B(f)}}
fn main(){let n=7;let mut calls=0;for first in [true,false]{let e=make(||{calls+=1;Some(&n)},first);let E.A(f)|E.B(f)=e else{panic("variant")};let Some(p)=f() else{panic("none")};if p.*!=7{panic("value")}};if calls!=2{panic("calls")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pattern_tuple_scalar() {
    let input = r#"
struct H<T>(fn() -> T)
fn make<T>(x:T) -> H<T> { H(|| x) }
fn main() { let n=5; let H(f) = make(Some(&n)); let Some(p)=f() else { panic("inner") }; if p!=&n { panic("pointer") } }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pattern_go_nullable_none() {
    let input = r#"
import "go:runtime/debug"
fn main(){let outer=debug.Module{Path:"outer",Version:"",Sum:"",Replace:None};let debug.Module{Replace:None,..}=outer else{panic("some")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pattern_go_nullable_callback_as_guard() {
    let input = r#"
import "go:crypto/tls"
import "go:time"
fn main(){let mut calls=0;let h=tls.Config{Time:Some(||{calls+=1;time.Unix(7,0)}),..};match h{tls.Config{Time:Some(f) as opt,..} if opt.is_some()=>{if f().Unix()!=7{panic("time")};let Some(g)=opt else{panic("none")};let _=g()},_=>panic("missing")};if calls!=2{panic("callback")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pattern_go_nullable_nested_or() {
    let input = r#"
import "go:runtime/debug"
fn main(){let mut child=debug.Module{Path:"child",..};let h=debug.Module{Replace:Some(&child),..};let out=match (h,7){(debug.Module{Replace:Some(r),..},0)|(debug.Module{Replace:Some(r),..},7)=>r.Path,_=>"none"};if out!="child"{panic("or")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn container_declaration_alias_tuple_projection() {
    let input = r#"
fn use_f(f:fn(int)->()){f(3)}
type P<T> = (fn(int)->T,int)
fn make<T>(f:fn(int)->T)->P<T>{(f,7)}
fn main(){let mut n=0;let p=make(|x|{n+=x});let f=p.0;use_f(f);if n!=3||p.1!=7{panic("count")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn container_declaration_review_r18_ref_receiver_before_argument() {
    let input = r#"
type Maybe<T> = Option<fn() -> Option<T>>
fn main() {
 let mut calls = 0
 let mut a: Maybe<fn() -> int> = Some(|| Some(|| 743))
 let r = &a
 let change = || { calls += 1; a = Some(|| Some(|| 751)); 0 }
 let n = r.map_or(change(), |f| { let Some(g) = f() else { panic("inner") }; g() })
 verify(n == 743 && calls == 1)
 let Some(f) = a else { panic("outer") }
 let Some(g) = f() else { panic("inner") }
 verify(g() == 751)
}
fn verify(ok: bool) { if !ok { panic("assertion failed") } }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_declaration_review_nested_storage() {
    let input = r#"
import "go:errors"
type F<T> = fn(int) -> T
type Nested<T> = (Option<F<T>>, Result<F<T>, string>)
type Alias<T> = Nested<T>
struct Box<T> { value: Alias<T> }
fn check(ok: bool) { if !ok { panic("runtime assertion failed") } }
fn source(n: int) -> Result<int, error> { if n > 0 { Ok(n + 1) } else { Err(errors.New("fail")) } }
fn make(n: int) -> Alias<Result<int, error>> { if n == 0 { return (None, Err("none")) }; (Some(source), Ok(|x| source(x))) }
fn forward(x: Alias<Result<int, error>>) -> Nested<Result<int, error>> { x }
fn boxed(b: Box<Result<int, error>>) -> Alias<Result<int, error>> { b.value }
fn main() {
  let empty = make(0)
  let (none, err) = empty
  let None = none else { panic("expected none") }
  let Err(text) = err else { panic("expected err") }
  check(text == "none")
  let pair = make(1)
  let stored = Box { value: pair }
  let (o, r) = forward(boxed(stored))
  let Some(f) = o else { panic("missing f") }
  let Ok(g) = r else { panic("missing g") }
  check(f(5).unwrap_or(0) == 6)
  check(g(8).unwrap_or(0) == 9)
  check(f(0).is_err())
  check(g(0).is_err())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_declaration_review_fallible_return_flow() {
    let input = r#"
import "go:errors"
type F<T> = fn(int) -> T
type R = Result<F<Option<int>>, error>
type O = Option<F<Option<int>>>
fn check(ok: bool) { if !ok { panic("runtime assertion failed") } }
fn source(n: int) -> Option<int> { if n > 0 { Some(n) } else { None } }
fn int_result(n: int) -> Result<int, error> { if n == 0 { Err(errors.New("empty")) } else { Ok(n) } }
fn make_r(n: int) -> R { let _ = int_result(n)?; Ok(source) }
fn make_o(n: int) -> O { let _ = source(n)?; Some(source) }
fn try_r(n: int) -> R { try { let _ = int_result(n)?; source } }
fn try_o(n: int) -> O { try { let _ = source(n)?; source } }
fn main() {
  let Ok(r) = make_r(1) else { panic("missing result") }
  check(r(2).unwrap_or(0) == 2)
  let Some(o) = make_o(1) else { panic("missing option") }
  check(o(3).unwrap_or(0) == 3)
  let Ok(tr) = try_r(1) else { panic("missing try result") }
  check(tr(4).unwrap_or(0) == 4)
  let Some(to) = try_o(1) else { panic("missing try option") }
  check(to(5).unwrap_or(0) == 5)
  let Err(_) = make_r(0) else { panic("expected result failure") }
  let None = make_o(0) else { panic("expected option failure") }
  let Err(_) = try_r(0) else { panic("expected try result failure") }
  let None = try_o(0) else { panic("expected try option failure") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_field_unit_ref_alias_mutation() {
    let input = r#"
struct H<T> { f: fn(int) -> T }
fn replace<T>(f: mut Ref<fn(int) -> T>, replacement: fn(int) -> T) { f.* = replacement }
fn main() { let mut n = 0
 let mut h: H<()> = H { f: |x| { n += x } }
 replace<()>( &h.f, |x| { n += 10 * x } )
 h.f(3)
 if n != 30 { panic("reference alias") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_generic_ref_identity() {
    let input = r#"
fn check(ok:bool){if !ok {panic("assertion failed")}}
fn id<T>(f:T)->T { f }
fn main(){let mut f=|x:int| (x,x+1); let mut r=id(&f); r.* = |x:int| (x+2,x+3); let p=f(4); check(p.0==6 && p.1==7)}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_index_order() {
    let input = r#"
fn index(calls: mut Ref<int>) -> int { calls.* += 1; 0 }
fn main() {
 let mut n = 0
 let mut calls = 0
 let mut fs: mut Slice<fn()> = [|| { n += 1 }]
 let mut r = &fs[index(&calls)]
 r.* = || { n += 10 }
 fs[0]()
 if n != 10 || calls != 1 { panic("indexed storage evaluation") }
 let saved = &fs[0]
 fs[0] = || { n += 100 }
 saved.*()
 if n != 110 { panic("replacement visibility") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_reference_function_field_storage() {
    let input = r#"
struct H{f:fn(int)}
fn main(){let mut n=0;let mut h=H{f:|x|{n+=x}};let mut r=&h.f;r.* = |x|{n+=10*x};h.f(3);if n!=30{panic("field storage")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_reference_function_index_storage() {
    let input = r#"
fn main(){let mut n=0;let mut fs:mut Slice<fn(int)> = [|x|{n+=x}];let mut r=&fs[0];r.* = |x|{n+=10*x};fs[0](3);if n!=30{panic("index storage")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_reference_function_local_storage() {
    let input = r#"
fn main(){let mut n=0;let mut f:fn(int)=|x|{n+=x};let mut r=&f;r.* = |x|{n+=10*x};f(3);if n!=30{panic("local storage")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_reference_function_observes_replacement() {
    let input = r#"
fn main(){let mut n=0;let mut f:fn(int)=|x|{n+=x};let r=&f;f=|x|{n+=10*x};r.*(3);if n!=30{panic("read storage")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_storage_reference_generic_field_storage() {
    let input = r#"
struct H<T>{f:fn(int)->T}
fn replace<T>(f:mut Ref<fn(int)->T>,g:fn(int)->T){f.* = g}
fn main(){let mut n=0;let mut h:H<()> = H{f:|x|{n+=x}};let mut r=&h.f;replace<()>(r,|x|{n+=10*x});h.f(3);if n!=30{panic("generic field storage")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_method_bound() {
    let input = r#"
type Maybe<T> = Option<T>
type Outcome<T> = Result<T, string>
fn main() {
 let o: Maybe<int> = None
 let missing = o.is_none
 let r: Outcome<int> = Err("x")
 let failed = r.is_err
 if !missing() || !failed() { panic("bound alias methods") }
 let mut calls = 0
 let n = o.unwrap_or_else(|| { calls += 1; 7 })
 if n != 7 || calls != 1 { panic("fallback evaluation") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_method_r4_or_else_direct() {
    let input = r#"
type Maybe<T> = Option<fn() -> T>
fn source() -> Option<int> { Some(43) }
fn inspect(input: Maybe<Option<int>>) {
  let restored = input.or_else(|| Some(source))
  match restored {
    Some(callback) => { if callback().unwrap_or(0) != 43 { panic("or_else callback payload") } },
    None => panic("or_else missing callback"),
  }
}
fn main() { inspect(None) }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_record_guard() {
    let input = r#"
interface Marker { fn code() -> int }
enum Event { Value { item: int }, Empty }
impl Event { fn code(self) -> int { 7 } }
fn get(v: Marker, count: mut Ref<int>) -> int {
 match v {
  Event.Value { item } if { count.* += 1; item > 5 } => item,
  Event.Value { item } => item + 10,
  _ => -1,
 }
}
fn main() {
 let mut count = 0
 let first = get(Event.Value { item: 3 }, &count)
 let empty = get(Event.Empty, &count)
 let last = get(Event.Value { item: 8 }, &count)
 if first != 13 || empty != -1 || last != 8 || count != 2 { panic("guard and variant checks") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_alias_callback_inferred() {
    let input = r#"
type Callback<T> = fn() -> T
enum Event<T> { Value { call: Callback<T> }, Empty }
fn wrap<T>(f: Callback<T>) -> Event<T> { Event.Value { call: f } }
fn main() {
 let mut n = 0
 let value = wrap(|| { n += 1 })
 match value { Event.Value { call } => call(), Event.Empty => () }
 if n != 1 { panic("wrapped callback") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn named_callback_review_direct_reference() {
    let input = r#"
import "go:go/ast"
import "go:go/doc"

fn replace(slot: mut Ref<doc.Filter>, next: doc.Filter) { slot.* = next }
fn returned(slot: mut Ref<ast.Filter>) -> mut Ref<doc.Filter> { slot }
fn main() {
  let mut hits = 0
  let mut original: ast.Filter = |s: string| { hits += 1; s == "before" }
  let view: mut Ref<doc.Filter> = &original
  if !(view.*)("before") { panic("initial callback changed") }
  replace(view, |s: string| { hits += 10; s == "after" })
  if !original("after") { panic("reference write detached") }
  let mut returned_view = returned(&original)
  replace(returned_view, |s: string| { hits += 100; s == "last" })
  if !original("last") { panic("returned reference detached") }
  if hits != 111 { panic("closure effects changed") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn named_callback_review_immutable_containers() {
    let input = r#"
import "go:go/ast"
import "go:go/doc"

fn option(value: Option<ast.Filter>) -> Option<doc.Filter> { value }
fn pair(value: (ast.Filter, Option<ast.Filter>)) -> (doc.Filter, Option<doc.Filter>) { value }
fn fallible(value: Result<ast.Filter, string>) -> Result<doc.Filter, string> { value }
fn main() {
  let mut hits = 0
  let callback: ast.Filter = |s: string| { hits += 1; s == "ok" }
  let present = option(Some(callback))
  let Some(f) = present else { panic("some became none") }
  if !f("ok") { panic("option callback changed") }
  if !option(None).is_none() { panic("none became some") }
  let converted = pair((callback, Some(callback)))
  if !(converted.0)("ok") { panic("tuple callback changed") }
  let Some(g) = converted.1 else { panic("nested option lost") }
  if !g("ok") { panic("nested callback changed") }
  let Ok(h) = fallible(Ok(callback)) else { panic("ok became err") }
  if !h("ok") { panic("result callback changed") }
  let Err(message) = fallible(Err("sentinel")) else { panic("err became ok") }
  if message != "sentinel" || hits != 4 { panic("effects or error changed") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn named_callback_review_nil_and_factory() {
    let input = r#"
import "go:go/ast"
import "go:go/doc"
import "go:reflect"

fn named(value: ast.Filter) -> doc.Filter { value }
fn plain(value: ast.Filter) -> fn(string) -> bool { value }
fn main() {
  let Some(zero) = assert_type<ast.Filter>(reflect.Zero(reflect.TypeFor<ast.Filter>()).Interface()) else { panic("nil callback type assertion failed") }
  let nil_named = named(zero)
  if !reflect.ValueOf(nil_named).IsNil() { panic("named conversion made nil nonnil") }
  let nil_plain = plain(zero)
  if !reflect.ValueOf(nil_plain).IsNil() { panic("plain conversion made nil nonnil") }
  let mut made = 0
  let mut calls = 0
  let factory = || -> ast.Filter { made += 1; |s: string| { calls += 1; s == "ok" } }
  let first: doc.Filter = factory()
  let second = named(factory())
  if made != 2 || calls != 0 { panic("conversion effects changed") }
  if !first("ok") || !second("ok") { panic("callback changed") }
  if made != 2 || calls != 2 { panic("callback effects changed") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_unit_method_value() {
    let input = r#"
type Done = ()
interface Action<T> { fn Run() -> T }
interface Child { embed Action<Done> }
struct Counter { n: mut Ref<int> }
impl Counter { fn Run(self: mut Ref<Counter>) -> Done { self.n.* += 1; return () } }
fn apply<A: Child>(a: A) { let f = a.Run; let _ = f(); let _ = f() }
fn main() {
 let mut n = 0
 let mut c = Counter { n: &n }
 apply(&c)
 let f = c.Run
 f()
 if n != 3 { panic("bound method value count") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_unit_method_interfaces() {
    let input = r#"
import "go:fmt"
type Done = ()
type Completion = Done
interface Perform<T> { fn perform() -> T }
interface Plain { fn perform() -> Completion }
fn invoke<T, A: Perform<T>>(a: A) -> T { a.perform() }
struct Worker { stop: bool }
impl Worker {
    fn perform(self) -> Completion {
        fmt.Print("enter,")
        if self.stop { return () }
        fmt.Print("tail,")
        ()
    }
}
fn through_interface(a: Plain) -> Completion { a.perform() }
fn through_callback(f: fn() -> Completion) -> Completion { f() }
fn main() {
    let early = Worker { stop: true }
    let normal = Worker { stop: false }
    through_interface(early)
    through_interface(normal)
    through_callback(early.perform)
    invoke<Completion, Worker>(normal)
    let iface: Plain = early
    iface.perform()
    fmt.Println("done")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn shared_callback_go_alias_reference() {
    let input = r#"
import "go:context"
type Cancel = context.CancelFunc
type Second = Cancel
fn view(f: mut Ref<Second>) -> mut Ref<context.CancelFunc> { f }
fn main() {
 let mut calls = 0
 let mut original: Second = || { calls += 1 }
 let mut forwarded = view(&original)
 forwarded.* = || { calls += 10 }
 original()
 if calls != 10 { panic("Go alias reference identity") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn shared_callback_record_array_callback_storage() {
    let input = r#"
fn use_f(f:fn(int)->()){f(3)}
struct H<T>{fs:Array<fn(int)->T,2>}
fn make<T>(f:fn(int)->T)->H<T>{H{fs:[f,f]}}
fn main(){let mut n=0;let h=make<()>(|x|{n+=x});let H{fs:[f,g]}=h;use_f(f);use_f(g);if n!=6{panic("count")}}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_state_alias_forward() {
    let input = r#"
import "go:errors"
type O = Option<int>
type R = Result<int, error>
type P = Partial<int, error>
type Pair = (int, Option<Ref<int>>)
fn check(ok: bool) { if !ok { panic("runtime assertion failed") } }
fn opt(n: int) -> O { if n > 0 { Some(n) } else { None } }
fn opt_explicit(n: int) -> O { return opt(n) }
fn opt_try(n: int) -> O { try { opt(n)? + 1 } }
fn res(n: int) -> R { if n > 0 { Ok(n) } else { Err(errors.New("negative")) } }
fn res_try(n: int) -> R { try { res(n)? + 1 } }
fn partial(n: int) -> P { match n { 0 => Partial.Err(errors.New("zero")), 1 => Partial.Ok(n), _ => Partial.Both(n, errors.New("both")) } }
fn partial_forward(n: int) -> P { partial(n) }
fn pair(n: int) -> Pair { let x = n + 1; (n, Some(&x)) }
fn main() {
  let passed_15 = match opt_explicit(4) { Some(n) => n == 4, _ => false }
  check(passed_15)
  let passed_16 = match opt_explicit(0) { None => true, _ => false }
  check(passed_16)
  let passed_17 = match opt_try(4) { Some(n) => n == 5, _ => false }
  check(passed_17)
  let passed_18 = match opt_try(0) { None => true, _ => false }
  check(passed_18)
  let passed_19 = match res_try(4) { Ok(n) => n == 5, _ => false }
  check(passed_19)
  let passed_20 = match res_try(0) { Err(_) => true, _ => false }
  check(passed_20)
  let passed_21 = match partial_forward(0) { Partial.Err(_) => true, _ => false }
  check(passed_21)
  let passed_22 = match partial_forward(1) { Partial.Ok(n) => n == 1, _ => false }
  check(passed_22)
  let passed_23 = match partial_forward(2) { Partial.Both(n, _) => n == 2, _ => false }
  check(passed_23)
  let (n, m) = pair(8)
  check(n == 8)
  let passed_26 = match m { Some(x) => x.* == 9, None => false }
  check(passed_26)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_state_generic_callback() {
    let input = r#"
import "go:errors"
fn choose<T>(value: T, mode: int) -> Partial<T, error> {
 if mode == 0 { Partial.Err(errors.New("err")) } else if mode == 1 { Partial.Ok(value) } else { Partial.Both(value, errors.New("both")) }
}
fn invoke<T>(f: fn() -> T) -> T { f() }
fn tag<T>(p: Partial<T,error>) -> int { match p { Partial.Err(_) => 0, Partial.Ok(_) => 1, Partial.Both(_,_) => 2 } }
fn main() {
 let mut calls = 0
 let f = || { calls += 1; choose((3, 4), 0) }
 if tag(invoke(f)) != 0 || calls != 1 { panic("generic callback Err") }
 if tag(choose((3, 4), 1)) != 1 || tag(choose((3, 4), 2)) != 2 { panic("generic Partial state") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_state_scalar() {
    let input = r#"
import "go:errors"
fn choose(mode: int) -> Partial<int, error> {
 if mode == 0 { return Partial.Err(errors.New("err")) }
 if mode == 1 { Partial.Ok(0) } else { Partial.Both(0, errors.New("both")) }
}
fn tag<T>(p: Partial<T,error>) -> int { match p { Partial.Err(_) => 0, Partial.Ok(_) => 1, Partial.Both(_,_) => 2 } }
fn main() {
 if tag(choose(0)) != 0 || tag(choose(1)) != 1 || tag(choose(2)) != 2 { panic("scalar Partial state") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_state_unit_forward() {
    let input = r#"
import "go:errors"
fn choose(mode: int) -> Partial<(), error> {
 match mode { 0 => Partial.Err(errors.New("err")), 1 => Partial.Ok(()), _ => Partial.Both((), errors.New("both")) }
}
fn forward(mode: int) -> Partial<(), error> { choose(mode) }
fn tag<T>(p: Partial<T,error>) -> int { match p { Partial.Err(_) => 0, Partial.Ok(_) => 1, Partial.Both(_,_) => 2 } }
fn main() {
 let f = forward
 if tag(f(0)) != 0 || tag(f(1)) != 1 || tag(f(2)) != 2 { panic("unit Partial state") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_review_alias_stored() {
    let input = r#"
import "go:io"

type Answer = Partial<int, error>
interface Base { fn fetch(fail: bool) -> Answer }
interface Child { embed Base }
type Face = Child
struct Source {}
impl Source {
  fn fetch(self, fail: bool) -> Answer {
    if fail { Partial.Err(io.EOF) } else { Partial.Both(0, io.EOF) }
  }
}
fn capture<T: Face>(source: T, fail: bool) -> Answer {
  let fetch = source.fetch
  fetch(fail)
}
fn main() {
  let both = capture(Source {}, false)
  if !both.is_both() { panic("lost Both tag") }
  let err = capture(Source {}, true)
  if !err.is_both() { panic("Err through a method reads back as Both") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_values_in_unknown_slots_keep_go_signatures() {
    let input = r#"
import "go:errors"
import "go:fmt"
fn fail(s: string) -> Result<string, error> { Err(errors.New("bad " + s)) }
fn find(_s: string) -> Option<int> { None }
fn unit() {}
fn kind(value: Unknown) -> string { fmt.Sprintf("%T", value) }
fn main() {
  let lambda = |_s: string| -> Result<int, error> { Ok(1) }
  let stored: Unknown = fail
  let values: Slice<Unknown> = [find, unit]
  if kind(stored) != "func(string) (string, error)" { panic("let binding") }
  if kind(lambda) != "func(string) (int, error)" { panic("local lambda") }
  if kind(values[0]) != "func(string) (int, bool)" { panic("option result") }
  if kind(values[1]) != "func()" { panic("unit result") }
  let Some(f) = assert_type<fn(string) -> Result<string, error>>(stored) else { panic("assert") }
  let Err(e) = f("x") else { panic("call after assert") }
  if e.Error() != "bad x" { panic("wrong error") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn template_funcs_reports_errors_from_lisette_functions() {
    let input = r#"
import "go:errors"
import "go:strings"
import "go:text/template"
fn fail(s: string) -> Result<string, error> { Err(errors.New("bad " + s)) }
fn main() {
  let mut funcs = Map.new<string, Unknown>()
  funcs["fail"] = fail
  let Ok(parsed) = template.New("x").Funcs(funcs).Parse("{{fail \"a\"}}") else { panic("parse") }
  let mut out = strings.Builder {}
  let Err(e) = parsed.Execute(&out, 0) else { panic("template swallowed the function error") }
  if !strings.Contains(e.Error(), "bad a") { panic("wrong error") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_alias_returns_through_go_callback_and_propagation() {
    let input = r#"
import "go:bufio"
import "go:errors"
import "go:strings"

type SplitOut = Result<(int, Slice<byte>), error>
type Done = Result<(), error>

fn split(data: Slice<byte>, at_eof: bool) -> SplitOut {
  let (n, token) = bufio.ScanWords(data, at_eof)?
  Ok((n, token))
}

fn fail() -> Done {
  Err(errors.New("boom"))?
  Ok(())
}

fn main() {
  let mut s = bufio.NewScanner(strings.NewReader("ab cd"))
  s.Split(split)
  let mut words = 0
  while s.Scan() { words += 1 }
  if words != 2 { panic("wrong word count") }
  let Err(_) = fail() else { panic("fail succeeded") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_methods_keep_the_type_visible_to_go_interfaces() {
    let input = r#"
import "go:fmt"
import "go:io"
import "go:strings"
struct Dev { id: int }
impl Dev {
  fn Read(self, _p: mut Slice<byte>) -> Partial<int, error> { Partial.Err(io.EOF) }
  fn WriteTo(self, w: io.Writer) -> Result<int64, error> {
    let _ = fmt.Fprint(w, "via WriteTo")
    Ok(0)
  }
  fn String(self) -> string { f"dev {self.id}" }
}
fn which(r: io.Reader) -> string {
  match r {
    Dev { id } => f"dev {id}",
    _ => "other",
  }
}
fn main() {
  let d = Dev { id: 7 }
  let r = d as io.Reader
  if which(r) != "dev 7" { panic("type switch lost the concrete type") }
  if fmt.Sprint(r) != "dev 7" { panic("fmt lost the Stringer") }
  let mut out = strings.Builder {}
  let _ = io.Copy(&out, r)
  if out.String() != "via WriteTo" { panic("io.Copy skipped WriteTo") }
}
"#;
    assert_emit_snapshot!(input);
}
