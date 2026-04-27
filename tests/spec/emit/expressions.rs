use crate::assert_emit_snapshot;

#[test]
fn binary_addition() {
    let input = r#"
fn test() -> int {
  1 + 2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_subtraction() {
    let input = r#"
fn test() -> int {
  10 - 5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_multiplication() {
    let input = r#"
fn test() -> int {
  3 * 4
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_division() {
    let input = r#"
fn test() -> int {
  20 / 4
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_modulo() {
    let input = r#"
fn test() -> int {
  10 % 3
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_equality() {
    let input = r#"
fn test() -> bool {
  5 == 5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_inequality() {
    let input = r#"
fn test() -> bool {
  5 != 3
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_less_than() {
    let input = r#"
fn test() -> bool {
  3 < 5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn less_than_negative_number() {
    let input = r#"
fn test() -> bool {
  let n = 5;
  n < -3
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_greater_than() {
    let input = r#"
fn test() -> bool {
  7 > 2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_less_than_or_equal() {
    let input = r#"
fn test() -> bool {
  5 <= 5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_greater_than_or_equal() {
    let input = r#"
fn test() -> bool {
  10 >= 5
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_logical_and() {
    let input = r#"
fn test() -> bool {
  true && false
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_logical_or() {
    let input = r#"
fn test() -> bool {
  true || false
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unary_negation() {
    let input = r#"
fn test() -> int {
  -42
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unary_logical_not() {
    let input = r#"
fn test() -> bool {
  !true
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_expression() {
    let input = r#"
fn test() -> int {
  (1 + 2) * 3
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_indexing() {
    let input = r#"
fn test(arr: Slice<int>) {
  arr[0]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_field_access() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) {
  p.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_zero_fill_lisette_primitives() {
    let input = r#"
struct Conf { name: string, count: int, on: bool }

fn test() -> Conf {
  Conf { name: "x", .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_zero_fill_lisette_option_emits_none() {
    let input = r#"
struct Conf { name: string, opt: Option<int> }

fn test() -> Conf {
  Conf { name: "x", .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_zero_fill_nested_user_struct_recurses() {
    let input = r#"
struct Inner { opt: Option<int>, items: Slice<int> }
struct Outer { inner: Inner }

fn test() -> Outer {
  Outer { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_zero_fill_lisette_slice_and_map_non_nil() {
    let input = r#"
struct Conf { items: Slice<int>, lookup: Map<string, int> }

fn test() -> Conf {
  Conf { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_zero_fill_enum_struct_variant() {
    let input = r#"
enum Action {
  Move { x: int, y: int, dist: int },
  Stop,
}

fn test() -> Action {
  Action.Move { x: 5, .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_field_access() {
    let input = r#"
struct Counter { value: int }

impl Counter {
  fn increment(self: Ref<Counter>) {
    self.*.value = self.*.value + 1
  }

  fn get(self: Ref<Counter>) -> int {
    self.*.value
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_access() {
    let input = r#"
fn test() {
  let tuple = (42, "hello");
  tuple.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_access_both_elements() {
    let input = r#"
fn test() -> int {
  let tuple = (42, "hello");
  tuple.0 + 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_field_assignment() {
    let input = r#"
fn test() -> int {
  let mut pair = (10, 20);
  pair.0 = 99;
  pair.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_simple() {
    let input = r#"
fn might_fail() -> Result<int, string> {
  Ok(42)
}

fn get_value() -> Result<int, string> {
  let result = might_fail()?;
  Ok(result)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_chained() {
    let input = r#"
fn first() -> Result<int, string> {
  Ok(10)
}

fn second() -> Result<int, string> {
  Ok(20)
}

fn test() -> Result<int, string> {
  let x = first()?;
  let y = second()?;
  Ok(x + y)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_in_expression() {
    let input = r#"
fn get_value() -> Result<int, string> {
    Ok(100)
}

fn test() -> Result<int, string> {
    Ok(get_value()?
 + 10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn const_simple() {
    let input = r#"
const MAX_SIZE = 100

fn test() -> int {
  MAX_SIZE
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn const_string() {
    let input = r#"
import "go:fmt"

const GREETING = "Hello, World!"

fn test() {
  fmt.Print(GREETING)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn const_expression() {
    let input = r#"
const RESULT = 10 + 20

fn test() -> int {
  RESULT * 2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn const_inside_function_body() {
    let input = r#"
fn main() {
  const MAX = 100
  let x = MAX + 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn const_go_keyword_name() {
    let input = r#"
import "go:fmt"

const range: int = 42

fn main() {
  fmt.Println(range)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn const_reference_to_const_stays_const() {
    let input = r#"
import "go:fmt"

const X = 10
const Y = X + 5

fn main() {
  fmt.Println(Y)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_simple() {
    let input = r#"
import "go:fmt"

fn test() {
  let name = "Alice";
  fmt.Print(f"Hello, {name}!")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_multiple() {
    let input = r#"
import "go:fmt"

fn test() {
  let x = 10;
  let y = 20;
  fmt.Print(f"x = {x}, y = {y}")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_variable() {
    let input = r#"
import "go:fmt"

fn test() {
  let result = 15;
  fmt.Print(f"Result: {result}")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_percent_sign() {
    let input = r#"
import "go:fmt"

fn test() {
  let pct = 50;
  fmt.Print(f"{pct}% complete")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_percent_no_interpolation() {
    let input = r#"
fn test() -> string {
  f"100% done"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_rune() {
    let input = r#"
import "go:fmt"

fn test() {
  let c = 'A';
  fmt.Print(f"char: {c}")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn compound_assignment() {
    let input = r#"
fn test() -> int {
  let mut x = 100;
  x += 10;
  x -= 5;
  x *= 2;
  x /= 3;
  x %= 7;
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_assignment() {
    let input = r#"
fn mutate(r: Ref<int>) {
  r.* = 99
}

fn test() -> int {
  let mut x = 1;
  mutate(&x);
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_self_in_ref_receiver() {
    let input = r#"
struct Foo { x: int }

impl Foo {
  fn copy_out(self: Ref<Foo>) -> Foo {
    let copy = self.*
    Foo { x: copy.x }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_assignment_target_captured_before_rhs() {
    let input = r#"
struct H { ptr: Ref<int> }

impl H {
  fn repoint(self: Ref<H>, q: Ref<int>) -> int {
    self.ptr = q
    9
  }
}

fn main() {
  let mut a = 1
  let mut b = 2
  let mut h = H { ptr: &a }

  h.ptr.* = h.repoint(&b)

  let _ = a
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn compound_deref_assignment_target_captured() {
    let input = r#"
struct H { ptr: Ref<int> }

impl H {
  fn repoint(self: Ref<H>, q: Ref<int>) -> int {
    self.ptr = q
    9
  }
}

fn main() {
  let mut a = 1
  let mut b = 2
  let mut h = H { ptr: &a }
  h.ptr.* += h.repoint(&b)
  let _ = a
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn dot_field_through_ref_assignment_captured() {
    let input = r#"
struct P { x: int }
struct H { ptr: Ref<P> }

impl H {
  fn repoint(self: Ref<H>, q: Ref<P>) -> int {
    self.ptr = q
    9
  }
}

fn main() {
  let mut a = P { x: 1 }
  let mut b = P { x: 2 }
  let mut h = H { ptr: &a }
  h.ptr.x = h.repoint(&b)
  let _ = a
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn binary_left_hoisted_when_right_is_call() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let z = i + bump(&i)
  let _ = z
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn call_args_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn sum(a: int, b: int) -> int { a + b }

fn main() {
  let mut i = 0
  let z = sum(i, bump(&i))
  let _ = z
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let t = (i, bump(&i))
  let _ = t
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_literal_field_eval_order_captured() {
    let input = r#"
struct Pair { a: int, b: int }

fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let p = Pair { a: i, b: bump(&i) }
  let _ = p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn format_string_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let s = f"{i},{bump(&i)}"
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_literal_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let s = [i, bump(&i)]
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_value_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let r = i..bump(&i)
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn regular_call_callee_eval_order_captured() {
    let input = r#"
fn f0(x: int) -> int { x + 10 }
fn f1(x: int) -> int { x + 20 }

fn bump(i: Ref<int>) -> int {
  i.* = 1
  0
}

fn main() {
  let mut i = 0
  let fs = [f0, f1]
  let r = fs[i](bump(&i))
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn index_access_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let items = [100, 200, 300]
  let mut i = 0
  let v = items[bump(&i)]
  let _ = v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let items = [10, 20, 30, 40, 50]
  let mut i = 1
  let s = items[i..bump(&i)]
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_receiver_method_eval_order_captured() {
    let input = r#"
struct Adder { base: int }

impl Adder {
  fn add(self, a: int, b: int) -> int {
    self.base + a + b
  }
}

fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let adder = Adder { base: 5 }
  let v = adder.add(i, bump(&i))
  let _ = v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_call_receiver_eval_order_captured() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn get(self) -> int { self.n }
}

fn bump_and_get(c: Ref<Counter>) -> int {
  c.*.n = c.*.n + 1
  c.*.n
}

fn sum(a: int, b: int) -> int { a + b }

fn main() {
  let mut c = Counter { n: 0 }
  let v = sum(c.get(), bump_and_get(&c))
  let _ = v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn native_method_dot_access_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let items = [10, 20, 30]
  let v = items.contains(bump(&i))
  let _ = v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn native_method_identifier_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> string {
  i.* = 1
  ","
}

fn main() {
  let mut i = 0
  let items = ["a", "b", "c"]
  let joined = items.join(bump(&i))
  let _ = joined
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_call_eval_order_captured() {
    let input = r#"
struct Pair(int, int)

fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let p = Pair(i, bump(&i))
  let _ = p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn append_args_eval_order_captured() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  10
}

fn main() {
  let mut i = 0
  let mut items: Slice<int> = []
  items.append(i, bump(&i))
  let _ = items
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn call_args_prebound_ref_eval_order() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = 1
  9
}

fn first(a: int, _b: int) -> int { a }

fn main() {
  let mut i = 0
  let m = [[10], [20]]
  let ip = &i
  let x = first(m[i][0], [bump(ip)][0])
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn call_args_carrier_struct_ref_eval_order() {
    let input = r#"
struct Carrier { p: Ref<int> }

fn bump(c: Carrier) -> int {
  c.p.* = 1
  9
}

fn first(a: int, _b: int) -> int { a }

fn main() {
  let mut i = 0
  let m = [[10], [20]]
  let c = Carrier { p: &i }
  let x = first(m[i][0], [bump(c)][0])
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assignment_deref_target_frozen_prebound_ref() {
    let input = r#"
struct Holder { p: Ref<int> }

fn retarget(h: Ref<Holder>, np: Ref<int>) -> int {
  h.p = np
  9
}

fn main() {
  let mut a = 1
  let mut b = 2
  let mut h = Holder { p: &a }
  let hp = &h
  let np = &b
  h.p.* = retarget(hp, np)
  let _ = a
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assignment_deref_target_frozen_carrier_ref() {
    let input = r#"
struct Holder { p: Ref<int> }
struct Carrier { h: Ref<Holder>, np: Ref<int> }

fn retarget(mut c: Carrier) -> int {
  c.h.p = c.np
  9
}

fn main() {
  let mut a = 1
  let mut b = 2
  let mut h = Holder { p: &a }
  let mut c = Carrier { h: &h, np: &b }
  h.p.* = retarget(c)
  let _ = a
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn call_args_carrier_enum_ref_eval_order() {
    let input = r#"
enum Carrier {
  C { p: Ref<int> },
}

fn bump(c: Carrier) -> int {
  match c {
    Carrier.C { p } => {
      p.* = 1
      9
    },
  }
}

fn first(a: int, _b: int) -> int { a }

fn main() {
  let mut i = 0
  let m = [[10], [20]]
  let c = Carrier.C { p: &i }
  let x = first(m[i][0], [bump(c)][0])
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assignment_deref_target_frozen_match_hidden_ref() {
    let input = r#"
struct Holder { p: Ref<int> }
struct Carrier { h: Ref<Holder>, np: Ref<int> }

fn retarget(c: Carrier) -> int {
  c.h.*.p = c.np
  9
}

fn main() {
  let mut a = 1
  let mut b = 2
  let mut h = Holder { p: &a }
  let c = Carrier { h: &h, np: &b }
  h.p.* = match 0 {
    0 => retarget(c),
    _ => 0,
  }
  let _ = a
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_pattern_match() {
    let input = r#"
fn test() -> int {
  let arr = [1, 2, 3];
  match arr {
    [a, b, c] => a + b + c,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_pattern_match_with_rest() {
    let input = r#"
fn test() -> int {
  let arr = [1, 2, 3, 4, 5];
  match arr {
    [first, ..rest] => first + rest[0],
    [] => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn named_function_as_value() {
    let input = r#"
fn test() -> int {
  let f = |x: int| -> int { x + 1 };
  f(5)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_vs_struct_disambiguation() {
    let input = r#"
struct Point { x: int }

fn test_if() -> int {
  let x = 1;
  if true { x } else { 0 }
}

fn test_struct() -> Point {
  let x = 42;
  Point { x }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn discard_unit_function_call() {
    let input = r#"
fn do_something() {
  // unit return
}

fn test() {
  let _ = do_something();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_shadows_go_builtin() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  let len = items.length();
  len
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parameter_shadows_go_builtin() {
    let input = r#"
fn test(len: int) -> int {
  len + 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_shadows_go_predeclared_type() {
    let input = r#"
import "go:strconv"

fn parse(s: string) -> Result<int, error> {
  let error = "context"
  let result = strconv.Atoi(s)
  match result {
    Ok(n) => Ok(n),
    Err(e) => Err(e),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_discard_empty_block() {
    let input = r#"
fn test() {
  let _ = {}
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_discard_shadowed_unit_struct() {
    let input = r#"
struct Blank {}

fn test() {
  let _ = Blank {}
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_enum_definition_in_function_body() {
    let input = r#"
enum Color { Red, Green, Blue }

fn main() {
  let c = Color.Red
  let _ = match c {
    Color.Red => 1,
    Color.Green => 2,
    Color.Blue => 3,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_index_chained_with_method_call() {
    let input = r#"
fn test() -> Option<int> {
  let items = ([1, 2, 3], [4, 5, 6])
  items.0.get(0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn closure_tail_expr_captures_mutable() {
    let input = r#"
fn make_counter(start: int) -> fn() -> int {
  let mut n = start;
  || {
    n += 1;
    n
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_index_assign_closure_type_propagation() {
    let input = r#"
fn test() {
  let mut m: Map<string, fn(int) -> int> = Map.new()
  m["square"] = |x| x * x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_then_index() {
    let input = r#"
fn first(items: Ref<Slice<int>>) -> int {
  items.*[0]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_then_slice() {
    let input = r#"
fn rest(items: Ref<Slice<int>>) -> Slice<int> {
  items.*[1..]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_map_value_method_call() {
    let input = r#"
struct Counter { count: int }

impl Counter {
  fn increment(self: Ref<Counter>) {
    self.*.count += 1
  }
}

fn test(m: Map<string, Ref<Counter>>) {
  m["a"].*.increment()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn address_of_enum_variant() {
    let input = r#"
enum List {
  Cons(int, Ref<List>),
  Nil,
}

fn test() -> List {
  List.Cons(42, &List.Nil)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn address_of_function_call() {
    let input = r#"
struct Foo { value: int }

fn make_foo() -> Foo {
  Foo { value: 42 }
}

fn takes_ref(f: Ref<Foo>) -> int {
  f.value
}

fn test() -> int {
  let tmp = make_foo()
  takes_ref(&tmp)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_address_function_call_receiver() {
    let input = r#"
struct Foo { value: int }

impl Foo {
  fn increment(self: Ref<Foo>) {
    self.value = self.value + 1
  }
}

fn make_foo() -> Foo {
  Foo { value: 42 }
}

fn test() {
  make_foo().increment()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_address_parenthesized_function_call_receiver() {
    let input = r#"
struct Foo { value: int }

impl Foo {
  fn increment(self: Ref<Foo>) {
    self.value = self.value + 1
  }
}

fn make_foo() -> Foo {
  Foo { value: 42 }
}

fn test() {
  (make_foo()).increment()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_address_struct_literal_receiver() {
    let input = r#"
struct Foo { value: int }

impl Foo {
  fn increment(self: Ref<Foo>) {
    self.value = self.value + 1
  }
}

fn test() {
  Foo { value: 42 }.increment()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_address_parenthesized_struct_literal_receiver() {
    let input = r#"
struct Foo { value: int }

impl Foo {
  fn increment(self: Ref<Foo>) {
    self.value = self.value + 1
  }
}

fn test() {
  (Foo { value: 42 }).increment()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_address_empty_struct_literal_receiver() {
    let input = r#"
struct Foo {}

impl Foo {
  fn need_ref(self: Ref<Foo>) {}
}

fn test() {
  Foo {}.need_ref()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_map_index_assignment() {
    let input = r#"
fn update(m: Ref<Map<string, int>>, key: string, val: int) {
  m.*[key] = val
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn user_var_named_subject_no_collision() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let subject_1 = 42
  let val = match opt {
    Some(v) => v,
    None => 0,
  }
  subject_1 + val
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_expr_same_name_as_let_binding() {
    let input = r#"
fn extract(opt: Option<int>) -> int {
  let x = match opt {
    Some(x) => x,
    None => 0,
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expr_same_name_as_let_binding() {
    let input = r#"
fn test() -> int {
  let result = {
    let result = 42;
    result * 2
  }
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_expr_same_name_as_let_binding() {
    let input = r#"
fn test() -> int {
  let val = if true {
    let val = 100;
    val + 1
  } else {
    0
  }
  val
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn some_unit_value() {
    let input = r#"
fn test() -> Option<()> {
  Some(())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assignment_in_let_discard_block() {
    let input = r#"
fn test() {
  let mut x = 0
  let _ = {
    x = 10
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn call_through_deref_ref_fn() {
    let input = r#"
type IntFn = fn(int) -> int

fn apply(f: Ref<IntFn>, x: int) -> int {
  f.*(x)
}

fn test() -> int {
  let double: IntFn = |x: int| x * 2;
  apply(&double, 3)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_struct_field_assignment() {
    let input = r#"
struct Player {
  name: string,
  score: int,
}

fn test() {
  let mut players = Map.new<string, Player>()
  players["alice"] = Player { name: "Alice", score: 0 }
  let mut entry = players["alice"]
  entry.score = 100
  players["alice"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_map_struct_field_assignment() {
    let input = r#"
struct Inner {
  value: int,
}

struct Outer {
  inner: Inner,
  name: string,
}

fn test() {
    let mut m = Map.new<string, Outer>()
    m["test"] = Outer { inner: Inner { value: 0 }, name: "test" }
    let mut entry = m["test"]
    entry.name = "updated"
    entry.inner.value = 42
    m["test"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_struct_field_assignment_go_keyword() {
    let input = r#"
struct Config {
  range: int,
  name: string,
}

fn test() {
    let mut m = Map.new<string, Config>()
    m["a"] = Config { range: 10, name: "first" }
    let mut entry = m["a"]
    entry.range = 20
    m["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_as_last_expression_in_block() {
    let input = r#"
import "go:fmt"

fn test() {
  let _ = {
    let _ = fmt.Println("")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_loop_as_last_expression_in_block() {
    let input = r#"
fn test() {
  let _ = {
    let mut i = 0
    while i < 3 {
      i = i + 1
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_as_last_expression_in_block() {
    let input = r#"
fn test() {
  let _ = {
    for x in [1, 2, 3] {
      let _ = x
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_index_evaluated_once() {
    let input = r#"
import "go:fmt"

fn make_range() -> Range<int> {
  fmt.Println("range")
  1..3
}

fn main() {
  let items = [0, 1, 2, 3, 4]
  let sub = items[make_range()]
  fmt.Println(sub.length())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_expression_position() {
    let input = r#"
import "go:fmt"

struct Config {
  value: int,
}

fn main() {
  let mut m: Map<string, Config> = Map.new()
  m["a"] = Config { value: 1 }
  let _ = {
    let mut entry = m["a"]
    entry.value = 2
    m["a"] = entry
  }
  fmt.Println(m["a"].value)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn declare_result_var_tracked_as_declared() {
    let input = r#"
fn main() {
  let _ = if true { 1 } else { 2 }
  let tmp_1 = 3
  let _ = tmp_1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn emit_or_capture_no_collision_with_user_vars() {
    let input = r#"
import "go:fmt"

fn main() {
  let _bound_0 = 10
  for i in 0..(1 + 2) {
    fmt.Println(i)
  }
  fmt.Println(_bound_0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_field_assignment_expression_position() {
    let input = r#"
import "go:fmt"

struct New(int)

fn main() {
  let mut n = New(1)
  let _ = { n = New(2) }
  fmt.Println(n.0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_dot_access_key_evaluated_once() {
    let input = r#"
import "go:fmt"

struct Key { k: string }
struct Box { x: int }

fn make_key(counter: Ref<int>) -> Key {
  counter.* = counter.* + 1
  Key { k: "a" }
}

fn main() {
  let mut counter = 0
  let mut m = Map.from([("a", Box { x: 1 })])
  let key = make_key(&counter).k
  let mut entry = m[key]
  entry.x = 2
  m[key] = entry
  fmt.Println(counter)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_key_evaluated_once() {
    let input = r#"
import "go:fmt"

struct C { value: int }

fn make_key() -> string {
  fmt.Println("key")
  "a"
}

fn main() {
  let mut m: Map<string, C> = Map.new()
  m["a"] = C { value: 1 }
  let key = make_key()
  let mut entry = m[key]
  entry.value = 2
  m[key] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_function_value_result_wrapper() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn parse_with(f: fn(string) -> Result<int, error>, s: string) -> int {
  match f(s) {
    Ok(n) => n,
    Err(_) => 0,
  }
}

fn main() {
  let n = parse_with(strconv.Atoi, "42")
  fmt.Println(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_temp_var_no_collision() {
    let input = r#"
import "go:fmt"

struct Box { x: int }

fn main() {
  let mut m = Map.from([("a", Box { x: 1 })])
  let mut entry = m["a"]
  entry.x = 2
  m["a"] = entry
  let entry_1 = 3
  fmt.Println(entry_1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_on_deref_map() {
    let input = r#"
struct Box { x: int }

fn main() {
  let mut m = Map.from([("k", Box { x: 0 })])
  let r = &m
  let mut entry = r.*["k"]
  entry.x = 1
  r.*["k"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_map_expr_evaluated_once() {
    let input = r#"
import "go:fmt"

struct Box { x: int }

fn make_i() -> int {
  fmt.Println("i")
  0
}

fn main() {
  let mut maps: Map<int, Map<string, Box>> = Map.new()
  let mut inner = Map.new<string, Box>()
  inner["k"] = Box { x: 0 }
  maps[0] = inner
  let i = make_i()
  let mut entry = maps[i]["k"]
  entry.x = 1
  maps[i]["k"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_tuple_field() {
    let input = r#"
import "go:fmt"

fn main() {
  let mut m = Map.new<string, (int, int)>()
  m["k"] = (0, 0)
  let mut entry = m["k"]
  entry.0 = 1
  m["k"] = entry
  let v = m["k"].0
  fmt.Println(v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_parenthesized() {
    let input = r#"
struct Box { x: int }

fn main() {
  let mut m = Map.from([("a", Box { x: 1 })])
  let mut entry = m["a"]
  entry.x = 2
  m["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_newtype() {
    let input = r#"
struct New(int)

fn main() {
  let mut m: Map<string, New> = Map.new()
  m["a"] = New(0)
  m["a"] = New(5)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_newtype_inner_field() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn main() {
  let mut m = Map.new<string, Wrap>()
  m["a"] = Wrap(Inner { x: 1 })
  let mut inner = m["a"].0
  inner.x = 2
  m["a"] = Wrap(inner)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assignment_double_newtype() {
    let input = r#"
struct Inner { x: int }
struct A(Inner)
struct B(A)

fn main() {
  let mut m = Map.new<string, B>()
  m["k"] = B(A(Inner { x: 1 }))
  let mut inner = m["k"].0.0
  inner.x = 2
  m["k"] = B(A(inner))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append() {
    let input = r#"
struct Box {
  items: Slice<int>,
}

fn main() {
  let mut m = Map.new<string, Box>()
  m["a"] = Box { items: [] }
  let mut entry = m["a"]
  entry.items = entry.items.append(1)
  m["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append_tuple_field() {
    let input = r#"
fn main() {
  let mut m = Map.new<string, (Slice<int>, int)>()
  m["a"] = ([1], 2)
  let mut entry = m["a"]
  entry.0 = entry.0.append(3)
  m["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append_tuple_struct_field() {
    let input = r#"
struct Wrap(Slice<int>, int)

fn main() {
  let mut m = Map.new<string, Wrap>()
  m["a"] = Wrap([], 0)
  let mut entry = m["a"]
  entry.0 = entry.0.append(2)
  m["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append_newtype() {
    let input = r#"
struct Wrap(Slice<int>)

fn main() {
  let mut m = Map.new<string, Wrap>()
  m["a"] = Wrap([])
  m["a"] = Wrap(m["a"].0.append(1))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append_newtype_inner_field() {
    let input = r#"
struct Inner {
  items: Slice<int>,
}
struct Wrap(Inner)

fn main() {
  let mut m = Map.new<string, Wrap>()
  m["a"] = Wrap(Inner { items: [] })
  let mut inner = m["a"].0
  inner.items = inner.items.append(1)
  m["a"] = Wrap(inner)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append_map_expr_evaluated_once() {
    let input = r#"
struct Box {
  items: Slice<int>,
}

fn get_map(m: Map<string, Box>) -> Map<string, Box> {
  m
}

fn main() {
  let mut m = Map.new<string, Box>()
  m["a"] = Box { items: [] }
  let mut map = get_map(m)
  let mut entry = map["a"]
  entry.items = entry.items.append(1)
  map["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_entry_slice_append_deref_map() {
    let input = r#"
struct Box {
  items: Slice<int>,
}

fn main() {
  let mut m = Map.new<string, Box>()
  m["a"] = Box { items: [] }
  let r = &m
  let mut entry = r.*["a"]
  entry.items = entry.items.append(1)
  r.*["a"] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_to_newtype_field() {
    let input = r#"
import "go:fmt"

struct Wrap(int)

fn main() {
  let w = Wrap(1)
  let inner = w.0
  let r = &inner
  fmt.Println(r.*)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn lvalue_capturing_tuple_field() {
    let input = r#"
import "go:fmt"

fn make_i() -> int {
  fmt.Println("i")
  0
}

fn main() {
  let mut items = [(0, 0)]
  items[make_i()].0 = if true { 1 } else { 2 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_temp_var_no_collision() {
    let input = r#"
import "go:fmt"

struct Box { x: int }

fn make_box() -> Box {
  Box { x: 1 }
}

fn main() {
  let r = &make_box()
  let ref_1 = 3
  fmt.Println(r.*.x, ref_1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_ptr_temp_var_no_collision() {
    let input = r#"
import "go:fmt"

enum Node {
  Leaf { value: int },
  Branch { left: Node, right: Node },
}

fn main() {
  let n = Node.Branch { left: Node.Leaf { value: 1 }, right: Node.Leaf { value: 2 } }
  let ptr_1 = 3
  fmt.Println(n)
  fmt.Println(ptr_1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_multi_return_ret_temp_var_no_collision() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let n = strconv.Atoi("42")
  let ret_1 = 7
  fmt.Println(n)
  fmt.Println(ret_1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_tuple_ptr_temp_var_no_collision() {
    let input = r#"
import "go:fmt"

enum Node {
  Leaf(int),
  Branch(Node, Node),
}

fn main() {
  let n = Node.Branch(Node.Leaf(1), Node.Leaf(2))
  let ptr_1 = 3
  fmt.Println(n)
  fmt.Println(ptr_1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn auto_address_receiver_ref_temp_var_no_collision() {
    let input = r#"
struct Box { x: int }

impl Box {
  fn inc(self: Ref<Box>) -> int {
    self.*.x = self.*.x + 1
    self.*.x
  }
}

fn make_box() -> Box {
  Box { x: 1 }
}

fn main() {
  let v = make_box().inc()
  let ref_1 = 7
  let _ = v
  let _ = ref_1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_subject_temp_var_no_collision() {
    let input = r#"
fn main() {
  let opt = Some(1);
  match opt {
    Some(x) => { let _ = x; },
    None => { let _ = 0; },
  }
  let subject_1 = 7;
  let _ = subject_1;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn emit_or_capture_range_loop_bound_temp_no_collision() {
    let input = r#"
fn make_bound() -> int { 3 }

fn main() {
  let mut sum = 0;
  for i in 0..make_bound() {
    sum = sum + i;
  }
  let _bound_1 = 7;
  let _ = sum;
  let _ = _bound_1;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn emit_or_capture_range_temp_var_no_collision() {
    let input = r#"
fn make_range() -> Range<int> { 0..2 }

fn main() {
  let s = [1, 2, 3];
  let sub = s[make_range()];
  let range_1 = 7;
  let _ = sub;
  let _ = range_1;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_subject_temp_no_collision() {
    let input = r#"
fn main() {
  let opt = Some(1);
  let Some(x) = opt else { return; };
  let subject_1 = 7;
  let _ = x;
  let _ = subject_1;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn complex_pattern_temp_no_collision() {
    let input = r#"
fn main() {
  let (a, (b, c)) = (1, (2, 3));
  let tmp_1 = 7;
  let _ = a;
  let _ = b;
  let _ = c;
  let _ = tmp_1;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_call_to_receiver_method() {
    let input = r#"
struct Box { x: int }

impl Box {
  fn add(self, y: int) -> int { self.x + y }
}

fn make_box() -> Box { Box { x: 1 } }

fn main() {
  let v = Box.add(make_box(), 1);
  let _ = v;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_call_return_only_type_args() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn make<U>(self) -> U { panic("boom") }
}

fn main() {
  let b = Box { value: 1 };
  let x: string = b.make();
  let _ = x;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_receiver_method_deref_parenthesized() {
    let input = r#"
struct Box { x: int }

impl Box {
  fn add(self, y: int) -> int { self.x + y }
}

fn make_ref() -> Ref<Box> { &Box { x: 1 } }

fn main() {
  let _ = Box.add(make_ref().*, 1);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn eval_order_assignment_call_target_if_value() {
    let input = r#"
fn get_items() -> Slice<int> { [1, 2, 3] }

fn main() {
  let mut items = [0, 0, 0]
  items[get_items()[0]] = if true { 42 } else { 0 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn eval_order_no_hoist_when_no_temps() {
    let input = r#"
fn a() -> int { 1 }
fn b() -> int { 2 }

fn main() {
  let x = a() + b()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn address_of_function_identifier() {
    let input = r#"
fn add1(x: int) -> int { x + 1 }

fn main() {
  let f = &add1
  let _ = f
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn method_expression_as_value_public() {
    let input = r#"
struct Box { x: int }

impl Box {
  pub fn add(self, y: int) -> int { self.x + y }
}

fn main() {
  let f = Box.add
  let _ = f(Box { x: 1 }, 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn static_method_as_value() {
    let input = r#"
struct Box { x: int }

impl Box {
  fn new(x: int) -> Box { Box { x } }
}

fn main() {
  let f = Box.new
  let _ = f(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn cast_in_statement_position_uses_discard() {
    let input = r#"
fn main() {
  let x = 1
  x as float64
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_unused_binding() {
    let input = r#"
fn main() {
  let ch = Channel.new<int>()
  select {
    match ch.receive() {
      Some(_) => { let _ = 0; },
      None => { let _ = 1; },
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send_value_eval_order() {
    let input = r#"
fn make_ch() -> Channel<int> { Channel.new<int>() }

fn main() {
  let to_send = if true { 1 } else { 0 }
  select {
    make_ch().send(to_send) => { let _ = 0; },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_complex_pattern_binding_nothing() {
    let input = r#"
struct Foo { x: int }

fn main() {
  let items = [Foo { x: 1 }, Foo { x: 2 }]
  for Foo { x: _ } in items {
    let _ = 0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_for_loop_end_before_start() {
    let input = r#"
fn start() -> int { 0 }
fn bound() -> int { 3 }

fn main() {
  for i in start()..bound() {
    let _ = i
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_for_loop_binding_before_start() {
    let input = r#"
fn main() {
  let start = 0
  for start in start..3 {
    let _ = start
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_start_after_end_temps() {
    let input = r#"
fn start() -> int { 0 }
fn items() -> Slice<int> { [1, 2, 3] }

fn main() {
  let end = if true { 2 } else { 3 }
  let s = items()[start()..end]
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assignment_lvalue_side_effects_with_temp_rhs() {
    let input = r#"
fn make_a() -> int { 0 }
fn make_b() -> int { 1 }

fn main() {
  let mut s = [0, 0, 0]
  s[make_a()] = if true { make_b() } else { 0 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_append_side_effecting_receiver() {
    let input = r#"
fn make_i() -> int { 0 }

fn main() {
  let mut outer = [[1], [2], [3]]
  outer[make_i()] = outer[make_i()].append(4)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_nested_parens_no_hoist() {
    let input = r#"
struct S { x: int }

impl S {
  fn inc(self: Ref<S>) { self.x = self.x + 1 }
}

fn main() {
  let mut s = S { x: 1 }
  let r = &((s))
  r.inc()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_go_function_value_wrapping() {
    let input = r#"
import "go:strconv"

fn main() {
  let r: Ref<fn(string) -> Result<int, error>> = &strconv.Atoi
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_address_of_receiver_parens() {
    let input = r#"
struct Counter { x: int }

impl Counter {
  fn inc(self: Ref<Counter>) -> int {
    self.x = self.x + 1
    self.x
  }
}

fn main() {
  let c = Counter { x: 0 }
  let _ = Counter.inc(&c)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn private_field_not_capitalized_by_method_export() {
    let input = r#"
pub interface IFoo {
  fn foo(self) -> int
}

struct S {
  foo: int,
}

fn main() {
  let s = S { foo: 1 }
  let _ = s.foo
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_dot_assignment_on_call_result() {
    let input = r#"
struct Box { x: int }

fn main() {
  let mut b = Box { x: 1 }
  let get = || &b
  get().*.x = 2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_append_parenthesized_receiver() {
    let input = r#"
fn main() {
  let mut s = [1]
  s = (s).append(2)
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_slice_append_statement_rewrite() {
    let input = r#"
fn main() {
  let mut s = [1]
  let r: Ref<Slice<int>> = &s
  r.* = r.*.append(2)
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn eval_order_assignment_index_target_if_value() {
    let input = r#"
fn main() {
  let mut items = [10, 20]
  let mut i = 0
  items[i] = if true { i = 1; 9 } else { 0 }
  let _ = items
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_nested_tuple_field_assignment() {
    let input = r#"
struct Pair(int, int)
struct Wrap(Pair)

fn main() {
  let mut w = Wrap(Pair(0, 0))
  let mut p = w.0
  p.1 = 9
  w = Wrap(p)
  let _ = w.0.1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deref_field_assignment_parens() {
    let input = r#"
struct Item { x: int }

fn main() {
  let mut v = Item { x: 0 }
  let r = &v
  r.*.x = 1
  let _ = v.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_nested_field_assign_eval_order() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn bump(i: Ref<int>) -> int {
  i.* = i.* + 1
  i.*
}

fn main() {
  let mut items = [Wrap(Inner { x: 0 })]
  let mut i = 0
  let mut inner = items[i].0
  inner.x = if true { bump(&i) } else { 0 }
  items[i] = Wrap(inner)
  let _ = items
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_nested_field_ref() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn main() {
  let mut w = Wrap(Inner { x: 1 })
  let inner = w.0
  let r = &inner.x
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_nested_field_assign_expr_position() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn main() {
  let mut w = Wrap(Inner { x: 0 })
  let mut inner = w.0
  let _ = { inner.x = 1 }
  w = Wrap(inner)
  let _ = w.0.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_field_assign_ref_expr_position() {
    let input = r#"
struct Wrap(int)

fn main() {
  let mut w = Wrap(0)
  let r = &w
  let _ = { r.* = Wrap(1) }
  let _ = w.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn append_non_lvalue_receiver_discards() {
    let input = r#"
struct Items { items: Slice<int> }

fn get() -> Items { Items { items: [1] } }

fn main() {
  get().items.append(2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_call_field_assignment() {
    let input = r#"
struct Item { x: int }

fn get() -> Ref<Item> { &Item { x: 0 } }

fn main() {
  get().x = 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_tuple_field_append() {
    let input = r#"
struct Pair(Slice<int>, int)
struct Wrap(Pair)

fn main() {
  let mut w = Wrap(Pair([1], 0))
  let mut p = w.0
  p.0 = p.0.append(2)
  w = Wrap(p)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_assign_captures_key() {
    let input = r#"
struct Pair { x: int }

fn set_key(k: Ref<string>) -> int {
  k.* = "b"
  1
}

fn main() {
  let mut m = Map.new<string, Pair>()
  m["a"] = Pair { x: 0 }
  let mut k = "a"
  let captured_key = k
  let mut entry = m[captured_key]
  entry.x = set_key(&k)
  m[captured_key] = entry
  let _ = m
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_newtype_slice_append() {
    let input = r#"
struct Wrap(Slice<int>)

fn main() {
  let mut w = Wrap([1])
  let r = &w
  r.* = Wrap(r.0.append(2))
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_ref_slice_append() {
    let input = r#"
struct Outer { items: Ref<Slice<int>> }

fn main() {
  let mut items = [1]
  let mut m = Map.new<string, Outer>()
  m["a"] = Outer { items: &items }
  m["a"].items.* = m["a"].items.*.append(2)
  let _ = items
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_ref_newtype_field_assign() {
    let input = r#"
struct Wrap(int)

fn main() {
  let mut w = Wrap(1)
  let mut m = Map.new<string, Ref<Wrap>>()
  m["a"] = &w
  m["a"].* = Wrap(2)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_ref_newtype_nested_field_assign() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn main() {
  let mut w = Wrap(Inner { x: 0 })
  let mut m = Map.new<string, Ref<Wrap>>()
  m["a"] = &w
  let mut inner = m["a"].0
  inner.x = 1
  m["a"].* = Wrap(inner)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_ref_newtype_slice_append() {
    let input = r#"
struct Wrap(Slice<int>)

fn main() {
  let mut w = Wrap([1])
  let mut m = Map.new<string, Ref<Wrap>>()
  m["a"] = &w
  m["a"].* = Wrap(m["a"].0.append(2))
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_ref_newtype_nested_field_append() {
    let input = r#"
struct Inner { items: Slice<int> }
struct Wrap(Inner)

fn main() {
  let mut w = Wrap(Inner { items: [1] })
  let mut m = Map.new<string, Ref<Wrap>>()
  m["a"] = &w
  let mut inner = m["a"].0
  inner.items = inner.items.append(2)
  m["a"].* = Wrap(inner)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_field_ref_slice_append_expr_position() {
    let input = r#"
struct Outer { items: Ref<Slice<int>> }

fn main() {
  let mut items = [1]
  let mut m = Map.new<string, Outer>()
  m["a"] = Outer { items: &items }
  let _ = { m["a"].items.*.append(2) }
  let _ = items
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_ref_newtype_mid_chain_field_assign() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)
struct Outer { w: Ref<Wrap> }

fn main() {
  let mut w = Wrap(Inner { x: 0 })
  let mut m = Map.new<string, Outer>()
  m["a"] = Outer { w: &w }
  let mut inner = m["a"].w.0
  inner.x = 2
  m["a"].w.* = Wrap(inner)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_ref_newtype_mid_chain_field_append() {
    let input = r#"
struct Inner { items: Slice<int> }
struct Wrap(Inner)
struct Outer { w: Ref<Wrap> }

fn main() {
  let mut w = Wrap(Inner { items: [1] })
  let mut m = Map.new<string, Outer>()
  m["a"] = Outer { w: &w }
  let mut inner = m["a"].w.0
  inner.items = inner.items.append(2)
  m["a"].w.* = Wrap(inner)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn append_non_lvalue_preserves_args() {
    let input = r#"
import "go:fmt"

fn get() -> Slice<int> { [1] }
fn arg() -> int { let _ = fmt.Println(1); 2 }

fn main() {
  get().append(arg())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_call_append_no_double_eval() {
    let input = r#"
import "go:fmt"

fn get() -> Ref<Slice<int>> {
  let _ = fmt.Println("get")
  let mut s = [1]
  &s
}

fn arg() -> int { let _ = fmt.Println("arg"); 2 }

fn main() {
  get().*.append(arg())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_call_newtype_append_no_double_eval() {
    let input = r#"
import "go:fmt"

struct Wrap(Slice<int>)

fn get() -> Ref<Wrap> {
  let _ = fmt.Println("get")
  let mut w = Wrap([1])
  &w
}

fn main() {
  get().0.append(2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_call_newtype_nested_assign_no_double_eval() {
    let input = r#"
import "go:fmt"

struct Inner { x: int }
struct Wrap(Inner)

fn get() -> Ref<Wrap> {
  let _ = fmt.Println("get")
  let mut w = Wrap(Inner { x: 1 })
  &w
}

fn main() {
  let r = get()
  let mut inner = r.0
  inner.x = 2
  r.* = Wrap(inner)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_call_field_assign_eval_order() {
    let input = r#"
import "go:fmt"

struct S { x: int }

fn get() -> Ref<S> {
  let _ = fmt.Println("get")
  let mut s = S { x: 1 }
  &s
}

fn arg() -> int { let _ = fmt.Println("arg"); 2 }

fn main() {
  get().x = if arg() > 0 { 3 } else { 4 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn expr_position_assign_preserves_lvalue() {
    let input = r#"
import "go:fmt"

fn idx() -> int { let _ = fmt.Println("idx"); 0 }
fn cond() -> bool { let _ = fmt.Println("cond"); true }

fn main() {
  let mut s = [1]
  let _ = { s[idx()] = if cond() { 2 } else { 3 } }
  let _ = fmt.Println(s[0])
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_unit_call_shadow() {
    let input = r#"
fn foo() { }

fn main() {
  let x = foo()
  let _ = x
  let x = foo()
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_over_ref_pointer_cast() {
    let input = r#"
struct Wrap(Ref<int>)

fn main() {
  let mut x = 1
  let w = Wrap(&x)
  let r = w.0
  let _ = r.*
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expr_append_map_entry_no_double_eval() {
    let input = r#"
struct Outer { items: Slice<int> }

fn key() -> string { "a" }

fn main() {
  let mut m = Map.new<string, Outer>()
  m["a"] = Outer{ items: [1] }
  let k = key()
  let mut entry = m[k]
  entry.items = entry.items.append(2)
  m[k] = entry
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expr_append_non_lvalue_captures_result() {
    let input = r#"
fn get() -> Slice<int> { [1] }
fn arg() -> int { 2 }

fn main() {
  let x = { get().append(arg()) }
  let _ = x[1]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expr_append_ref_slice_non_lvalue() {
    let input = r#"
fn get() -> Ref<Slice<int>> {
  let mut s = [1]
  &s
}
fn arg() -> int { 2 }

fn main() {
  let x = { get().*.append(arg()) }
  let _ = x[1]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_slice_append_statement_writeback() {
    let input = r#"
fn main() {
  let mut s = [1]
  s = Slice.append(s, 2)
  let _ = s[1]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_returning_map_delete_in_tuple() {
    let input = r#"
fn main() {
  let mut m = Map.new<string, int>()
  m["a"] = 1
  let t = (m.delete("a"), 1)
  let _ = t.1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_returning_call_in_tuple() {
    let input = r#"
fn do_nothing() {}

fn main() {
  let t = (42, do_nothing())
  let _ = t.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expr_unit_returning_call() {
    let input = r#"
fn do_nothing() {}

fn main() {
  let x = { do_nothing() }
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expr_append_ref_newtype() {
    let input = r#"
struct Wrap(Slice<int>)

fn get() -> Ref<Wrap> {
  let mut w = Wrap([1])
  &w
}

fn main() {
  let x = { get().0.append(2) }
  let _ = x[0]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_slice_append_expr_position() {
    let input = r#"
fn main() {
  let mut s = [1]
  let r = &s
  let _ = r.*.append(2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_newtype_append_expr_position() {
    let input = r#"
struct Wrap(Slice<int>)

fn main() {
  let mut w = Wrap([1])
  let r = &w
  let _ = r.0.append(2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_newtype_extend_expr_position() {
    let input = r#"
struct Wrap(Slice<int>)

fn main() {
  let mut w = Wrap([1])
  let r = &w
  let _ = r.0.extend([2])
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_slice_append() {
    let input = r#"
fn noop() {}

fn test() {
  let mut xs: Slice<()> = []
  xs.append(noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_format_string() {
    let input = r#"
fn noop() {}

fn test() -> string {
  f"x:{noop()}"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_as_assignment_rhs() {
    let input = r#"
fn noop() {}

fn test() {
  let mut x: () = ()
  x = noop()
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_binary_equality() {
    let input = r#"
fn noop() {}

fn test() {
  let b = noop() == ()
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_identifier_native_append() {
    let input = r#"
fn noop() {}

fn main() {
  let xs: Slice<()> = []
  let y = Slice.append(xs, noop())
  let _ = y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_assert_type() {
    let input = r#"
fn noop() {}

fn main() {
  let x = assert_type<()>(noop())
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_as_map_index_key() {
    let input = r#"
fn noop() {}

fn main() {
  let mut m = Map.new<(), int>()
  m[()] = 7
  let x = m[noop()]
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_as_map_index_key_assignment() {
    let input = r#"
fn noop() {}

fn main() {
  let mut m = Map.new<(), int>()
  m[noop()] = 7
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_unit_call_statement() {
    let input = r#"
fn noop() {}

fn main() {
  (noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_discard_parenthesized_unit_call() {
    let input = r#"
fn noop() {}

fn main() {
  let _ = (noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_parenthesized_unit_call() {
    let input = r#"
fn noop() {}

fn main() {
  let x = (noop())
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_defer_statement() {
    let input = r#"
fn noop() {}

fn main() {
  (defer noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_task_statement() {
    let input = r#"
fn noop() {}

fn main() {
  (task { noop() })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_call_base_field_assignment() {
    let input = r#"
struct S { x: int }

fn get(r: Ref<S>) -> Ref<S> { r }

fn main() {
  let mut s = S { x: 0 }
  (get(&s)).x = 1
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_to_unit_returning_call() {
    let input = r#"
fn noop() {}

fn main() {
  let _ = &noop()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unused_mutable_variable_reassignment() {
    let input = r#"
fn test() {
  let mut x = 0
  x = 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unused_mutable_variable_multiple_reassignments() {
    let input = r#"
fn f() -> int { 42 }
fn g() -> int { 99 }

fn test() {
  let mut x = 0
  x = f()
  x = g()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn static_method_returning_channel_not_hijacked() {
    let input = r#"
struct Foo { ch: Channel<int> }

impl Foo {
  fn new() -> Channel<int> {
    Channel.new<int>()
  }
}

fn test() -> Channel<int> {
  Foo.new()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn free_function_new_returning_channel_not_hijacked() {
    let input = r#"
fn new() -> Channel<int> {
  Channel.new<int>()
}

fn test() -> Channel<int> {
  new()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_binding_imaginary_not_hijacked() {
    let input = r#"
fn id(x: int) -> int { x }

fn test() -> int {
  let imaginary = id
  imaginary(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_binding_new_not_hijacked() {
    let input = r#"
fn mk() -> int { 42 }

fn test() -> int {
  let new = mk
  new()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_builtin_imaginary_call() {
    let input = r#"
fn test(c: complex128) -> float64 {
  (imaginary)(c)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_native_ufcs_call() {
    let input = r#"
fn main() {
  let mut xs = [1]
  xs = (Slice.append)(xs, 2)
  let _ = xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_native_instance_method_call() {
    let input = r#"
fn test() -> int {
  let xs = [1, 2, 3]
  (xs.length)()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_tuple_struct_call() {
    let input = r#"
struct Point(int, int)

fn test() -> Point {
  (Point)(1, 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parenthesized_generic_callee_call() {
    let input = r#"
fn id<T>(x: T) -> T { x }

fn test() -> int {
  (id)(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_multi_field_tuple_struct_constructor() {
    let input = r#"
struct Point2D(int, int)
type P = Point2D

fn test() -> P {
  P(1, 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_generic_tuple_struct_constructor() {
    let input = r#"
struct Wrapper<T>(T)
type W<T> = Wrapper<T>

fn test() -> W<int> {
  W(5)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_of_call_in_call_args() {
    let input = r#"
fn make_int() -> int { 1 }
fn read_ref(r: Ref<int>) -> int { r.* }
fn side_effect() -> int { 1 }

fn test() -> int {
  side_effect() + read_ref(&make_int())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_of_call_nested_enum_constructors() {
    let input = r#"
enum Expr {
  Num(int),
  Add(Ref<Expr>, Ref<Expr>),
}

fn test() -> Expr {
  Expr.Add(&Expr.Num(1), &Expr.Num(2))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_of_call_no_hoist_when_no_side_effects() {
    let input = r#"
fn takes_two(a: int, b: Ref<int>) -> int { a + b.* }
fn make_int() -> int { 2 }

fn test() -> int {
  takes_two(1, &make_int())
}
"#;
    assert_emit_snapshot!(input);
}
