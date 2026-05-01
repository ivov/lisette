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
interface Printable {
  fn to_string(self) -> string
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
fn put_in_map<K, V>(key: K, value: V) -> Map<K, V> {
  let mut m: Map<K, V> = Map.new()
  m[key] = value
  m
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct_with_map_field_comparable_constraint() {
    let input = r#"
struct Cache<K, V> {
  data: Map<K, V>,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_array_return_temp_var_no_collision() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let data = "hello" as Slice<uint8>
  let hash = sha256.Sum256(data)
  let arr_1 = 7
  let _ = hash
  let _ = arr_1
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
fn go_some_for_interface_parameter() {
    let input = r#"
import "go:net/http"
import "go:strings"
import "go:fmt"

fn main() {
  let body = strings.NewReader("hello")
  let req = http.NewRequest("POST", "https://example.com", Some(body))
  fmt.Println(req)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_with_mut_parameter() {
    let input = r#"
fn process(mut items: Slice<int>) {
  items = [1, 2, 3]
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

fn make_map<T>(t: T) -> int {
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
fn use_map_in_task<K, V>(key: K, value: V) {
  task {
    let mut m: Map<K, V> = Map.new()
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

fn make(counter: Ref<int>) -> Ref<bytes.Buffer> {
  counter.* = counter.* + 1
  bytes.NewBufferString("a\nb\n")
}

fn use_fn(f: fn(uint8) -> Result<string, error>) {
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
  filepath.Join(..parts)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_with_leading_args_into_go_variadic() {
    let input = r#"
import filepath "go:path/filepath"

fn test(base: string, rest: Slice<string>) -> string {
  filepath.Join(base, ..rest)
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
  l.push(..parts)
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
  Logger.push(l, ..parts)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_native_slice_append() {
    let input = r#"
fn test(s: Slice<int>, more: Slice<int>) -> Slice<int> {
  s.append(..more)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_into_native_slice_append_assignment() {
    let input = r#"
fn test(mut s: Slice<int>, more: Slice<int>) -> Slice<int> {
  s = s.append(..more)
  s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn spread_arg_with_leading_args_into_native_slice_append_assignment() {
    let input = r#"
fn test(mut s: Slice<int>, extra: int, more: Slice<int>) -> Slice<int> {
  s = s.append(extra, ..more)
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
  fmt.Println(..xs)
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
  Ok(variadic(side_a(), ..get_xs()?))
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
