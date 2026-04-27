use crate::assert_parse_snapshot;

#[test]
fn array_literal_operand() {
    let input = r#"
fn test() { let result = [1, 2, 3][index]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_addition() {
    let input = r#"
fn test() { let result = 10 + 5; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_subtraction() {
    let input = r#"
fn test() { let result = 10 - 5; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_multiplication() {
    let input = r#"
fn test() { let result = 10 * 5; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_division() {
    let input = r#"
fn test() { let result = 10 / 5; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_remainder() {
    let input = r#"
fn test() { let result = 10 % 3; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_equal() {
    let input = r#"
fn test() { let result = a == b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_not_equal() {
    let input = r#"
fn test() { let result = a != b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_less_than() {
    let input = r#"
fn test() { let result = a < b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_greater_than() {
    let input = r#"
fn test() { let result = a > b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_less_than_or_equal() {
    let input = r#"
fn test() { let result = a <= b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_greater_than_or_equal() {
    let input = r#"
fn test() { let result = a >= b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_and() {
    let input = r#"
fn test() { let result = true && false; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn binary_or() {
    let input = r#"
fn test() { let result = true || false; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_multiplication_addition() {
    let input = r#"
fn test() { let result = a + b * c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_addition_multiplication() {
    let input = r#"
fn test() { let result = a * b + c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_addition_equal() {
    let input = r#"
fn test() { let result = a + b == c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_equal_addition() {
    let input = r#"
fn test() { let result = a == b + c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_equal_and() {
    let input = r#"
fn test() { let result = a == b && c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_and_or() {
    let input = r#"
fn test() { let result = a && b || c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn precedence_unary_binary() {
    let input = r#"
fn test() { let result = !a && b; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn associativity_addition_subtraction() {
    let input = r#"
fn test() { let result = a + b - c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn associativity_multiplication_division() {
    let input = r#"
fn test() { let result = a * b / c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_override_multiplication_addition() {
    let input = r#"
fn test() { let result = (a + b) * c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_override_and_or() {
    let input = r#"
fn test() { let result = a && (b || c); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pipe_simple() {
    let input = r#"
fn test() { let result = x |> func; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pipe_chained() {
    let input = r#"
fn test() { let result = x |> f |> g |> h; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pipe_with_call() {
    let input = r#"
fn test() { let result = x |> add(5); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pipe_chained_with_calls() {
    let input = r#"
fn test() { let result = x |> add(5) |> multiply(2) |> subtract(3); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pipe_precedence_addition() {
    let input = r#"
fn test() { let result = a + b |> func; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pipe_precedence_or() {
    let input = r#"
fn test() { let result = a |> f || b |> g; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn associativity_pipe() {
    let input = r#"
fn test() { let result = a |> b |> c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_basic() {
    let input = r#"
fn test() {
  let x = 10;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_with_type() {
    let input = r#"
fn test() {
  let message: string = "hello";
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_mutable() {
    let input = r#"
fn test() {
  let mut counter = 0;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_mutable_with_type() {
    let input = r#"
fn test() {
  let mut active: bool = true;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_complex_value() {
    let input = r#"
fn test() {
  let result = (10 + 5) * 2 / calculate_something();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_struct_literal() {
    let input = r#"
fn test() {
  let p = Point { x: 1, y: 2 };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_destructure_tuple() {
    let input = r#"
fn test() {
  let (a, b): (int, string) = get_pair();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_destructure_tuple_inferred() {
    let input = r#"
fn test() {
  let (a, b) = get_pair();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_destructure_tuple_wildcard() {
    let input = r#"
fn test() {
  let (_, status_code): (string, int) = http_request();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_destructure_struct() {
    let input = r#"
fn test() {
  let Point { x, y: y_coord }: Point = get_point();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_else_basic() {
    let input = r#"
fn test() -> Option<int> {
  let Some(x) = opt else { return None; };
  Some(x)
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_else_with_result() {
    let input = r#"
fn test() -> int {
  let Ok(value) = get_result() else { return -1; };
  value
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_else_complex_pattern() {
    let input = r#"
fn test() {
  let (a, Some(b)) = tuple else { break; };
  use_values(a, b);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_else_with_break() {
    let input = r#"
fn test() {
  loop {
    let Some(item) = iter.next() else { break; };
    process(item);
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn let_else_with_continue() {
    let input = r#"
fn test() {
  for item in items {
    let Valid(data) = validate(item) else { continue; };
    process(data);
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn const_basic() {
    let input = r#"
const MAX_RETRIES = 5;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn const_with_type() {
    let input = r#"
const PI: float64 = 3.14159;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn const_string() {
    let input = r#"
const GREETING: string = "Hello";
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn assign_simple_var() {
    let input = r#"
fn test() {
  let mut count = 0;
  count = count + 1;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn assign_field() {
    let input = r#"
fn test() {
  let mut p = Point { x: 1, y: 1};
  p.x = 10;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn assign_index() {
    let input = r#"
fn test() {
  let mut arr = [1, 2, 3];
  arr[0] = 100;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn complex_field_and_index_chain() {
    let input = r#"
fn test() { let result = obj.field[i].method().nested.value; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn chained_method_calls() {
    let input = r#"
fn test() { let result = obj.method1().method2(); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn task_with_block() {
    let input = r#"
fn test() {
  task {
    print("concurrent task");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn task_with_expression() {
    let input = r#"
fn test() {
  task compute();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn select_receive() {
    let input = r#"
fn test(ch: Receiver<int>) {
  select {
    let x = ch => process(x),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn select_send() {
    let input = r#"
fn test(ch: Sender<int>) {
  select {
    ch.send(42) => print("sent"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn select_wildcard() {
    let input = r#"
fn test() {
  select {
    _ => print("default"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn select_multiple_arms() {
    let input = r#"
fn test(ch1: Receiver<int>, ch2: Sender<string>) {
  select {
    let x = ch1 => process(x),
    ch2.send("hello") => print("sent"),
    _ => print("default"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn select_match_receive() {
    let input = r#"
fn test(ch: Channel<int>) {
  select {
    match ch.receive() {
      Some(v) => process(v),
      None => handle_close(),
    },
    _ => default(),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_basic() {
    let input = r#"
enum Status {
  Pending,
  Processing,
  Complete,
  Failed,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_adt() {
    let input = r#"
enum IpAddress {
  V4(uint8, uint8, uint8, uint8),
  V6(string),
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_with_generics() {
    let input = r#"
enum Result<T, E> {
  Ok(T),
  Err(E),
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_trailing_comma() {
    let input = r#"
enum Color {
  Red,
  Green,
  Blue,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_empty() {
    let input = r#"
enum Nothing {}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_struct_variant() {
    let input = r#"
enum Message {
  Move { x: int, y: int },
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_struct_variant_single_field() {
    let input = r#"
enum Event {
  Click { x: int },
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_struct_variant_empty() {
    let input = r#"
enum Token {
  Eof {},
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_mixed_variants() {
    let input = r#"
enum Message {
  Quit,
  Move { x: int, y: int },
  Write(string),
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn field_access_chained() {
    let input = r#"
fn test(cfg: Config) {
  let username = cfg.auth.user.name;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn field_access_after_call() {
    let input = r#"
fn test() {
  let x_coord = get_point().x;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn field_access_method_call() {
    let input = r#"
fn test(user: User) {
  let name = user.get_name();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn field_access_tuple() {
    let input = r#"
fn test(pair: (int, string)) {
  let first_item = pair.0;
  let second_item = pair.1;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn format_string_simple() {
    let input = r#"
fn test() { let result = f"hello world"; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn format_string_with_variable() {
    let input = r#"
fn test() { let result = f"hello {name}"; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn format_string_with_field_access() {
    let input = r#"
fn test() { let result = f"name: {person.name}"; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn format_string_multiple() {
    let input = r#"
fn test() { let result = f"hello {first} {last}!"; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn function_basic() {
    let input = r#"
fn calculate(a: int, b: int) -> int {
  let result = a + b; // this is a comment
  if result > 10 {
    print("Large!\n");
  }
  return result;
}
"#;

    assert_parse_snapshot!(input);
}

#[test]
fn function_main() {
    let input = r#"
fn main() {
   let total = 7 + 5;
   let is_active = true;
   let pi = 3.14;
   let name = "Lisette";
   let initial = 'L';
   match total {
       12 => print("Correct!"),
       _  => print("Incorrect!"),
   }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn function_with_generic() {
    let input = r#"
 fn foo<T>(x: T) -> int {
    match x {
      1 => 1;
    }
  }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn function_with_empty_body() {
    let input = r#"
fn noop() {
  // does nothing
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn function_with_nested_function() {
    let input = r#"
fn outer() {
  fn inner(x: int) -> int {
    return x * 2;
  };

  inner(5);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn function_recursive() {
    let input = r#"
fn factorial(n: int) -> int {
  if n <= 1 {
    return 1;
  }
  return n * factorial(n - 1);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn function_with_multiple_returns() {
    let input = r#"
fn abs(x: int) -> int {
  if x < 0 {
    return -x;
  }
  return x;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pub_function() {
    let input = r#"
pub fn greet() { }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pub_enum() {
    let input = r#"
pub enum Status { Active, Inactive }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pub_struct() {
    let input = r#"
pub struct Point { x: int, y: int }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pub_const() {
    let input = r#"
pub const MAX: int = 100;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pub_type_alias() {
    let input = r#"
pub type Id = int;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn pub_interface() {
    let input = r#"
pub interface Printable { fn print(self); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_single_unbounded() {
    let input = r#"
fn identity<T>(x: T) -> T {
  return x;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_multiple_unbounded() {
    let input = r#"
fn pair<T, U>(x: T, y: U) -> (T, U) {
  return (x, y);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_single_bound() {
    let input = r#"
fn display<T: Display>(value: T) -> string {
  return "displayed";
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_multiple_params_with_bounds() {
    let input = r#"
fn compare<T: Display, U: Debug>(x: T, y: U) -> bool {
  return true;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_multiple_bounds() {
    let input = r#"
fn process<T: Display + Clone>(value: T) -> T {
  return value;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_mixed_bounds() {
    let input = r#"
fn complex<T: Display + Clone, U: Debug, V>(x: T, y: U, z: V) -> int {
  return 0;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_struct_unbounded() {
    let input = r#"
struct Box<T> {
  value: T,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_struct_bounded() {
    let input = r#"
struct Container<T: Clone> {
  item: T,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_enum_single() {
    let input = r#"
enum Option<T> {
  Some(T),
  None,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_enum_multiple() {
    let input = r#"
enum Result<T, E> {
  Ok(T),
  Err(E),
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_impl() {
    let input = r#"
impl<T> Box<T> {
  fn new(value: T) -> Box<T> {
    return Box { value };
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_simple() {
    let input = r#"
fn test(a: int) {
  if a > 10 {
    print("large");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_else() {
    let input = r#"
fn test(a: int) {
  if a > 10 {
    print("large");
  } else {
    print("small or medium");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_else_if() {
    let input = r#"
fn test(a: int) {
  if a > 100 {
    print("very large");
  } else if a > 10 {
    print("large");
  } else {
    print("small or medium");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_complex_condition() {
    let input = r#"
fn test(a: int, b: bool) {
  if (a * 2 > 50) && b || is_special(a) {
    // complex condition
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_nested() {
    let input = r#"
fn test(a: int, b: int) {
  if a > 0 {
    if b > 0 {
      print("both positive");
    } else {
      print("a positive, b non-positive");
    }
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_let_simple() {
    let input = r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    print(x);
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_let_else() {
    let input = r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    print(x);
  } else {
    print("none");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_let_else_if_let() {
    let input = r#"
fn test(a: Option<int>, b: Option<int>) {
  if let Some(x) = a {
    print(x);
  } else if let Some(y) = b {
    print(y);
  } else {
    print("both none");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn if_let_expression() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let result = if let Some(x) = opt {
    x * 2
  } else {
    0
  };
  result
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_simple() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn get(self: Counter) -> int {
    return self.value;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_static_method() {
    let input = r#"
struct Math {}

impl Math {
  fn add(a: int, b: int) -> int {
    return a + b;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_multiple_methods() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn get(self: Counter) -> int {
    return self.value;
  }

  fn increment(self: Counter) -> Counter {
    return Counter { value: self.value + 1 };
  }

  fn new() -> Counter {
    return Counter { value: 0 };
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_with_generics() {
    let input = r#"
struct Container<T> {
  value: T,
}

impl<T> Container<T> {
  fn get(self: Container<T>) -> T {
    return self.value;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_multiple_type_params() {
    let input = r#"
struct Pair<K, V> {
  key: K,
  value: V,
}

impl<K, V> Pair<K, V> {
  fn get_key(self: Pair<K, V>) -> K {
    return self.key;
  }

  fn get_value(self: Pair<K, V>) -> V {
    return self.value;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_empty() {
    let input = r#"
struct Empty {}

impl Empty {}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_method_with_params() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn add(self: Counter, amount: int) -> int {
    return self.value + amount;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_complex_return_type() {
    let input = r#"
struct Container<T> {
  items: Vec<T>,
}

impl<T> Container<T> {
  fn get_all(self: Container<T>) -> Vec<T> {
    return self.items;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_no_semicolons_after_methods() {
    let input = r#"
struct Point {
  x: int,
  y: int,
}

impl Point {
  fn new(x: int, y: int) -> Point {
    return Point { x, y };
  }
  fn get_x(self: Point) -> int {
    return self.x;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_method_bodies() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn complex_logic(self: Counter) -> int {
    let x = self.value + 1;
    let y = x * 2;
    if y > 10 {
      return y;
    }
    return x;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_pub_method() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  pub fn get(self: Counter) -> int {
    return self.value;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn impl_mixed_visibility_methods() {
    let input = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn private_helper(self: Counter) -> int {
    return self.value * 2;
  }

  pub fn get(self: Counter) -> int {
    return self.private_helper();
  }

  pub fn set(self: Counter, new_value: int) -> Counter {
    return Counter { value: new_value };
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_basic() {
    let input = r#"
import "my_module"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_go_stdlib() {
    let input = r#"
import "go:fmt"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_deeply_qualified() {
    let input = r#"
import "core/net/http"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_multiple() {
    let input = r#"
import "go:fmt"
import "app/services"
import "third/party/utils"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_mixed_with_function() {
    let input = r#"
import "go:io"
import "network"

fn process() {
  // function body
}

import "logging"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_named_alias() {
    let input = r#"
import router "go:github.com/gorilla/mux"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_blank() {
    let input = r#"
import _ "go:database/sql"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn import_mixed_aliases() {
    let input = r#"
import router "go:github.com/gorilla/mux"
import _ "go:database/sql"
import "go:fmt"
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn indexing_variable() {
    let input = r#"
fn test(arr: Slice<int>, i: int) {
  let val = arr[i];
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn indexing_map() {
    let input = r#"
fn test(data: Map<string, bool>) {
  let active = data["user_status"];
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn indexing_chained() {
    let input = r#"
fn test(matrix: Slice<Slice<int>>, row: int, col: int) {
  let cell = matrix[row][col];
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn indexing_after_call() {
    let input = r#"
fn test(key: string) {
  let value = get_map()[key];
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn nested_indexing() {
    let input = r#"
fn test() { let result = arr[i + j][k * l]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_no_params() {
    let input = r#"
fn test() {
  let f = || 42;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_single_param() {
    let input = r#"
fn test() {
  let double = |x| x * 2;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_multiple_params() {
    let input = r#"
fn test() {
  let add = |x, y| x + y;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_with_type_annotation() {
    let input = r#"
fn test() {
  let square = |x: int| -> int x * x;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_with_param_types() {
    let input = r#"
fn test() {
  let multiply = |a: int, b: int| a * b;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_as_argument() {
    let input = r#"
fn test(numbers: Slice<int>) {
  let result = map(numbers, |n| n * 2);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_with_block_body() {
    let input = r#"
fn test() {
  let process = |x| {
    let doubled = x * 2;
    doubled + 1
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_calling_immediately() {
    let input = r#"
fn test() {
  let result = (|x| x + 1)(5);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_nested() {
    let input = r#"
fn test() {
  let outer = |x| |y| x + y;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_in_return() {
    let input = r#"
fn test() {
  return |x| x * 2;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_complex_expression() {
    let input = r#"
fn test(items: Slice<int>) {
  let result = filter(items, |n| n > 0 && n < 100);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_capturing_variable() {
    let input = r#"
fn test() {
  let threshold = 10;
  let check = |x| x > threshold;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_no_params_after_let() {
    let input = r#"
fn make_counter() -> fn() -> int {
  let mut n = 0
  || -> int {
    n = n + 1
    n
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn lambda_no_params_expr_after_let() {
    let input = r#"
fn test() -> fn() -> int {
  let x = 42
  || x
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn loop_infinite() {
    let input = r#"
fn test() {
  loop {
    print("forever");
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn loop_with_break() {
    let input = r#"
fn test() {
  loop {
    if should_stop() {
      break;
    }
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn loop_with_break_value() {
    let input = r#"
fn test() {
  let x = loop {
    if done() {
      break 42;
    }
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn loop_with_continue() {
    let input = r#"
fn test() {
  loop {
    if skip_this() {
      continue;
    }
    process();
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn while_loop() {
    let input = r#"
fn test() {
  let mut count = 0;
  while count < 10 {
    print(count);
    count = count + 1;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn while_let_simple() {
    let input = r#"
fn test(items: Slice<Option<int>>) {
  let mut i = 0;
  while let Some(x) = items[i] {
    print(x);
    i = i + 1;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn while_let_tuple_pattern() {
    let input = r#"
fn test(pairs: Slice<Option<(int, int)>>) {
  let mut i = 0;
  while let Some((a, b)) = pairs[i] {
    print(a + b);
    i = i + 1;
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn for_loop() {
    let input = r#"
fn test(items: Slice<string>) {
  for item in items {
    print(item);
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn for_loop_tuple_binding() {
    let input = r#"
fn test(map_data: Map<string, int>) {
  for (key, value) in map_data {
    print(key, value);
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_literal() {
    let input = r#"
fn test(code: int) {
  match code {
    200 => print("OK"),
    404 => print("Not Found"),
    _   => print("Other"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_bool() {
    let input = r#"
fn test(active: bool) {
  match active {
    true  => start_service(),
    false => stop_service(),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_string() {
    let input = r#"
fn test(command: string) {
  match command {
    "start" => exec("start"),
    "stop"  => exec("stop"),
    _       => print("Unknown command"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_struct() {
    let input = r#"
fn test(p: Point) {
  match p {
    Point { x: 0, y: 0 } => print("Origin"),
    Point { x, y: 0 }    => print("On X axis", x),
    Point { x: 0, y }    => print("On Y axis", y),
    Point { x, y }       => print("Somewhere else", x, y),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_struct_uppercase_shorthand() {
    let input = r#"
fn test(p: Point) {
  match p {
    Point { X } => X,
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_nested_adt() {
    let input = r#"
fn test(opt_res: Option<Result<int, string>>) {
  match opt_res {
    Some(Ok(value)) => print("Success:", value),
    Some(Err(msg))  => print("Inner Error:", msg),
    None            => print("No value"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_nested_adt_with_wildcards() {
    let input = r#"
enum State { Running(int), Stopped, Error(string, Option<int>) }
fn test(state: State) {
  match state {
    State.Running(_)      => print("Running"),
    State.Stopped         => print("Stopped"),
    State.Error(_, None)  => print("Error without code"),
    State.Error(msg, Some(code)) => print("Error with code", msg, code),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_tuple_pattern() {
    let input = r#"
fn test(pair: (int, int)) {
  match pair {
    (0, 0) => print("Origin"),
    (x, 0) => print("On X axis", x),
    (0, y) => print("On Y axis", y),
    (x, y) => print("Coords", x, y),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_tuple_pattern_with_wildcard() {
    let input = r#"
fn test(triple: (string, int, bool)) {
  match triple {
    ("status", code, _) => print("Status code:", code),
    (_, _, true)        => print("Active"),
    _                   => print("Other"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_unit_pattern() {
    let input = r#"
fn test(val: ()) {
  match val {
    () => print("It's unit!"),
    // _ => print("Not unit?"), // unreachable for `()` subject
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn match_mixed_patterns() {
    let input = r#"
enum Data { Int(int), Pair((int, int)), Text(string), Nothing }
fn test(data: Data) {
  match data {
    Data.Int(0)      => print("Zero"),
    Data.Int(n)      => print("Number", n),
    Data.Pair((0, y))=> print("Pair on Y axis", y),
    Data.Pair(_)     => print("Some pair"),
    Data.Text("end") => print("End marker"),
    Data.Text(_)     => print("Some text"),
    Data.Nothing     => print("Nothing"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_simple_literal() {
    let input = r#"
fn test() { let x = (10); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_simple_variable() {
    let input = r#"
fn test(y: int) { let x = (y); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_precedence_override() {
    let input = r#"
fn test(a: int, b: int, c: int) { let result = (a + b) * c; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_nested() {
    let input = r#"
fn test(a: int, b: int, c: int, d: int) { let result = ((a + b) * c) - d; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_around_call() {
    let input = r#"
fn test() { let result = (calculate(5)); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_in_condition() {
    let input = r#"
fn test(a: int, b: int) {
  if (a > 0) && (b < 10) {
    // ...
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_in_return() {
    let input = r#"
fn test(a: int, b: int) -> int {
  return (a * b);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn paren_multiple_nested() {
    let input = r#"
fn test(w: int, x: int, y: int, z: int) {
  let result = (w + (x * (y - z)));
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn postfix_method_call() {
    let input = r#"
fn test() { let result = obj.method(); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn postfix_propagate_operator() {
    let input = r#"
fn test() { let result = expr?; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn postfix_tuple_field_access() {
    let input = r#"
fn test() { let result = tuple.0; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn postfix_expression_with_propagate() {
    let input = r#"
fn test() { let result = func()? + obj.method(a, b + c)?; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_simple() {
    let input = r#"
struct Point {
  x: int,
  y: int,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_generic() {
    let input = r#"
struct Container<T> {
  value: T,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_multiple_generics() {
    let input = r#"
struct Pair<K, V> {
  key: K,
  value: V,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_trailing_comma() {
    let input = r#"
struct Config {
  timeout: int,
  retries: int,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_complex_field_types() {
    let input = r#"
struct Request<Body> {
  method: string,
  headers: Map<string, string>,
  body: Option<Body>,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_empty() {
    let input = r#"
struct Empty {}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_pub_fields() {
    let input = r#"
struct Point {
  pub x: int,
  pub y: int,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_mixed_visibility_fields() {
    let input = r#"
struct Config {
  pub name: string,
  secret: string,
  pub enabled: bool,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_literal_shorthand() {
    let input = r#"
fn test(x: int, y: int) {
  let p = Point { x, y };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_literal_spread() {
    let input = r#"
fn test(defaults: Config) {
  let cfg = Config { enabled: true, ..defaults };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_literal_mixed() {
    let input = r#"
fn test(x: int, base: Point) {
  let p = Point { x, y: 10, ..base };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_literal_nested() {
    let input = r#"
fn test(user: User) {
  let data = Data {
    id: 1,
    payload: Payload { user, status: "active" },
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_literal_empty() {
    let input = r#"
fn test() {
  let e = Empty {};
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_literal_as_arg() {
    let input = r#"
fn test(x_val: int) {
  process(Point { x: x_val, y: 0 });
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_init_operand() {
    let input = r#"
fn test() { let result = Point { x: 1, y: 2 }.distance(); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn nested_struct_init() {
    let input = r#"
fn test() { let result = Outer { inner: Inner { x: 1, y: 2 }, z: 3 }; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_update_syntax() {
    let input = r#"
fn test() { let result = Point { x: 1, ..base_point }; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_shorthand_syntax() {
    let input = r#"
fn test() {
  let x = 10;
  let y = 20;
  let result = Point { x, y };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn generic_struct_instantiation() {
    let input = r#"
struct Container<T> { value: T }
fn test() {
  let x = 42;
  let c: Container<int> = Container { value: x };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_in_if_condition() {
    let input = r#"
struct Point { x: int }
fn test(other: Point) -> bool {
  let x = 1;
  if Point { x } == other { true } else { false }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_in_match_subject() {
    let input = r#"
struct Point { x: int }
fn test() -> int {
  let x = 42;
  match Point { x } {
    Point { x: val } => val,
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_simple() {
    let input = r#"
struct Point(int, int)
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_single_field() {
    let input = r#"
struct UserId(int)
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_with_generics() {
    let input = r#"
struct Wrapper<T>(T)
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_multiple_generics() {
    let input = r#"
struct Pair<A, B>(A, B)
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_trailing_comma() {
    let input = r#"
struct Point(int, int,)
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_complex_types() {
    let input = r#"
struct Container(Slice<int>, Map<string, bool>)
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn tuple_struct_zero_field() {
    let input = r#"
struct Marker()
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn brace_after_call_is_block() {
    let input = r#"
fn get_value() -> int { 1 }
fn test() {
  let x = 1;
  get_value() { x }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn brace_after_paren_expr_is_block() {
    let input = r#"
fn test() {
  let foo = 1;
  let x = 2;
  (foo) { x }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn empty_brace_after_lowercase_is_block() {
    let input = r#"
fn test() {
  let foo = 1;
  foo {}
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn empty_brace_after_uppercase_is_struct() {
    let input = r#"
struct Foo {}
fn test() {
  let result = Foo {};
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn empty_struct_in_call_in_control_flow_header() {
    let input = r#"
struct Data {}
fn foo(d: Data) -> Result<Data, Data> {
  Ok(d)
}
fn test() {
  if let Err(err) = foo(Data {}) { panic(err) }
  let _ = match foo(Data {}) {
    Ok(d) => d,
    Err(err) => panic(err)
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn brace_after_binary_is_block() {
    let input = r#"
fn test() {
  let a = 1;
  let b = 2;
  let x = 3;
  a + b { x }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn shorthand_struct_in_binary_rhs_allowed_outside_control_flow() {
    let input = r#"
fn test() {
  let foo = 1;
  let x = 3;
  foo < Bar { x }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn shorthand_struct_blocked_in_control_flow_header() {
    let input = r#"
fn test() {
  let foo = 1;
  let Bar = 2;
  let x = 3;
  if foo < Bar { x } else { 0 }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn explicit_struct_in_binary_rhs_allowed() {
    let input = r#"
struct Bar { x: int }
fn test() {
  let foo = 1;
  if foo < Bar { x: 42 } { true } else { false }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn spread_struct_in_binary_rhs_allowed() {
    let input = r#"
struct Bar { x: int }
fn test(base: Bar) {
  let foo = 1;
  if foo < Bar { ..base } { true } else { false }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn shorthand_struct_in_logical_rhs_allowed() {
    let input = r#"
struct Point { x: int }
fn test(other: Point) {
  let foo = true;
  let x = 1;
  if foo && Point { x } == other { true } else { false }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn shorthand_struct_in_comparison_rhs_with_body_after() {
    let input = r#"
struct Point { x: int }
fn test(other: Point) {
  let x = 1;
  if other == Point { x } { true } else { false }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn shorthand_struct_in_parenthesized_condition() {
    let input = r#"
struct Point { x: int }
fn test(other: Point) {
  let x = 1;
  if (Point { x } == other) { true } else { false }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn shorthand_struct_in_nested_if_condition() {
    let input = r#"
struct Point { x: int }
fn test(foo: int, other: Point) {
  let x = 1;
  let ok = foo < if Point { x } == other { 1 } else { 0 };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn propagate_simple() {
    let input = r#"
fn test(res: Result<int, string>) {
  let value = res?;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn propagate_after_call() {
    let input = r#"
fn test() {
  let data = load_data()?;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn propagate_in_chain() {
    let input = r#"
fn test() {
  let user = load_config()?.user;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn propagate_in_chain_multiple() {
    let input = r#"
fn test() {
  let name = load_config()?.user?.name;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_ref() {
    let input = r#"
type IntRef = Ref<int>;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_function() {
    let input = r#"
type Callback = fn(int, string) -> bool;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_slice() {
    let input = r#"
type IntSlice = Slice<int>;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_tuple() {
    let input = r#"
type Pair = (int, string);
type Unit = ();
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_qualified() {
    let input = r#"
type Data = Foo.Bar;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_generic_qualified() {
    let input = r#"
type Container = Foo.Bar<T>;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_nested_generics() {
    let input = r#"
type Complex = Result<Option<T>, Error>;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_alias_simple() {
    let input = r#"
type UserID = int;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_alias_generic() {
    let input = r#"
type StringMap<V> = Map<string, V>;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn type_alias_other_alias() {
    let input = r#"
type ID = int;
type ProductID = ID;
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_minus_literal() {
    let input = r#"
fn test() {
  let x = -10;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_minus_variable() {
    let input = r#"
fn test(count: int) {
  let negation_count = -count;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_minus_parenthesized() {
    let input = r#"
fn test(a: int, b: int) {
  let result = -(a + b);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_precedence_negation_add() {
    let input = r#"
fn test(a: int, b: int) {
  let result = -a + b;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_precedence_negation_multiplication() {
    let input = r#"
fn test(a: int, b: int) {
  let result = -a * b;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_precedence_negation_field() {
    let input = r#"
fn test(p: Point) {
  let negation_x = -p.x;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_not_literal() {
    let input = r#"
fn test() {
  let is_not_true = !true;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_not_variable() {
    let input = r#"
fn test(is_active: bool) {
  let is_inactive = !is_active;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_not_parenthesized() {
    let input = r#"
fn test(a: bool, b: bool) {
  let result = !(a && b);
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_precedence_not_and() {
    let input = r#"
fn test(a: bool, b: bool) {
  let result = !a && b;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_precedence_not_or() {
    let input = r#"
fn test(a: bool, b: bool) {
  let result = !a || b;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_precedence_not_call() {
    let input = r#"
fn test() {
  let result = !is_ready();
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_double_not() {
    let input = r#"
fn test(a: bool) {
  let still_a = !!a;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_double_negation() {
    let input = r#"
fn test(a: int) {
  let still_a = --a;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_not_negation() {
    let input = r#"
fn test(a: int) {
  let weird = !-a;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_with_postfix() {
    let input = r#"
fn test() { let result = !obj.method(); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_reference() {
    let input = r#"
fn test() { let result = &x; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_reference_complex() {
    let input = r#"
fn test() { let result = &(a + b).field; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn unary_reference_precedence() {
    let input = r#"
fn test() { let result = &1 + 2; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn directive_rawgo_valid() {
    let input = r#"
fn test() { @rawgo("fmt.Println(\"Hello\")"); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn directive_rawgo_invalid_arg() {
    let input = r#"
fn test() { @rawgo(123); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn directive_unknown() {
    let input = r#"
fn test() { @unknown; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn return_bare_followed_by_code() {
    let input = r#"
fn test() {
  return;
  let x = 5;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_spread_trailing_comma() {
    let input = r#"
fn test(base: Point) { let p = Point { ..base, }; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_zero_fill_with_fields() {
    let input = r#"
fn test() { let p = Point { x: 1, .. }; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_zero_fill_only() {
    let input = r#"
fn test() { let p = Point { .. }; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_zero_fill_trailing_comma() {
    let input = r#"
fn test() { let p = Point { x: 1, .., }; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_type_arg_simple() {
    let input = r#"
fn test() { func<int>(x); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_type_arg_nested() {
    let input = r#"
fn test() { func<Map<string, int>>(); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_type_args_multiple() {
    let input = r#"
fn test() { func<A, B, C>(x, y); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn method_call_with_type_args() {
    let input = r#"
fn test() { obj.method<int>(arg); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_function_type_in_type_args() {
    let input = r#"
fn test() { Map.new<string, fn(int, int) -> int>(); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn slice_pattern_empty() {
    let input = r#"
fn test(items: Slice<int>) {
  match items {
    [] => print("empty"),
    _ => print("not empty"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn slice_pattern_single() {
    let input = r#"
fn test(items: Slice<int>) {
  match items {
    [x] => print("single", x),
    _ => print("other"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn slice_pattern_with_rest() {
    let input = r#"
fn test(items: Slice<int>) {
  match items {
    [] => print("empty"),
    [first, ..rest] => print("head", first),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn slice_pattern_rest_only() {
    let input = r#"
fn test(items: Slice<int>) {
  match items {
    [..] => print("any slice"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn slice_pattern_suffix() {
    let input = r#"
fn test(items: Slice<int>) {
  match items {
    [..init, last] => print("last is", last),
    [] => print("empty"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn compound_assignment() {
    let input = r#"
fn test() {
  let mut x = 10;
  x += 5;
  x -= 3;
  x *= 2;
  x /= 4;
  x %= 3;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_exclusive() {
    let input = r#"
fn test() { let r = 0..10; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_inclusive() {
    let input = r#"
fn test() { let r = 0..=10; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_from() {
    let input = r#"
fn test() { for i in 0.. { break; } }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_to() {
    let input = r#"
fn test(arr: Slice<int>) { let head = arr[..3]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_to_inclusive() {
    let input = r#"
fn test(arr: Slice<int>) { let head = arr[..=3]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_full() {
    let input = r#"
fn test(arr: Slice<int>) { let copy = arr[..]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_in_for_loop() {
    let input = r#"
fn test() { for i in 0..5 { print(i); } }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_with_expressions() {
    let input = r#"
fn test(start: int, end: int) { let r = start..end; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_precedence_with_comparison() {
    let input = r#"
fn test(other: bool) { let r = 0..5 == other; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_precedence_with_arithmetic() {
    let input = r#"
fn test() { let r = 0..1 + 2; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn range_slice_indexing() {
    let input = r#"
fn test(arr: Slice<int>) { let sub = arr[1..4]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_simple() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | 2 | 3 => "small",
    _ => "other",
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_enum_variants() {
    let input = r#"
enum Color { Red, Green, Blue }
fn test(c: Color) -> string {
  match c {
    Red | Green => "warm",
    Blue => "cool",
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_with_bindings() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) | None => 0,
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_multiple_arms() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    0 | 1 => "binary",
    2 | 3 | 5 | 7 => "small prime",
    _ => "other",
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_strings() {
    let input = r#"
fn test(s: string) -> int {
  match s {
    "yes" | "y" | "true" => 1,
    "no" | "n" | "false" => 0,
    _ => -1,
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_booleans() {
    let input = r#"
fn test(a: bool, b: bool) -> string {
  match (a, b) {
    (true, true) | (false, false) => "same",
    (true, false) | (false, true) => "different",
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn or_pattern_parenthesized() {
    let input = r#"
enum Color { Red, Blue }
fn test(c: Color) -> int {
  match c {
    (Red | Blue) => 1,
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn try_block_basic() {
    let input = r#"
fn test() {
  let result = try {
    risky()?
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn try_block_multiple_statements() {
    let input = r#"
fn test() {
  let result = try {
    let a = get_a()?;
    let b = get_b()?;
    a + b
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn try_block_with_semicolon() {
    let input = r#"
fn test() {
  let result = try {
    do_something()?;
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn try_block_nested() {
    let input = r#"
fn test() {
  let outer = try {
    let inner = try {
      inner_risky()?
    };
    outer_risky()?
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn try_block_with_control_flow() {
    let input = r#"
fn test() {
  let result = try {
    for i in items {
      if i > 10 {
        break;
      }
    };
    get_value()?
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn value_enum_basic() {
    let input = r#"
enum Weekday {
  Sunday = 0,
  Monday = 1,
  Tuesday = 2,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn value_enum_hex() {
    let input = r#"
enum FileMode {
  ModeDir = 0x80000000,
  ModeRegular = 0,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn value_enum_string() {
    let input = r#"
enum HttpMethod {
  Get = "GET",
  Post = "POST",
  Put = "PUT",
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn value_enum_negative() {
    let input = r#"
enum Offset {
  Start = 0,
  End = -1,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn negative_pattern_i64_min() {
    let input = r#"
fn classify(x: int) -> string {
  match x {
    -9223372036854775808 => "min",
    _ => "other",
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn value_enum_negative_i64_min() {
    let input = r#"
enum Time: int64 {
  Earliest = -9223372036854775808,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_json_attribute() {
    let input = r#"
#[json]
struct User {
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_json_attribute_snake_case() {
    let input = r#"
#[json(snake_case)]
struct User {
  first_name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_field_with_json_attribute() {
    let input = r#"
#[json]
struct User {
  #[json(omitempty)]
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_field_with_name_override() {
    let input = r#"
#[json]
struct User {
  #[json("userName")]
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_field_with_skip() {
    let input = r#"
#[json]
struct User {
  #[json(skip)]
  password: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_multiple_attributes() {
    let input = r#"
#[json]
#[db]
struct User {
  #[json(omitempty)]
  #[db("user_name")]
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_tag_attribute_raw() {
    let input = r#"
#[tag]
struct User {
  #[tag(`json:"name,omitempty" validate:"required"`)]
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_tag_attribute_structured() {
    let input = r#"
#[tag]
struct User {
  #[tag("json", omitempty)]
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn struct_with_negated_flag() {
    let input = r#"
#[json(omitempty)]
struct User {
  #[json(!omitempty)]
  name: string,
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn recover_block_basic() {
    let input = r#"
fn test() {
  let result = recover { foo() };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn recover_block_multiple_statements() {
    let input = r#"
fn test() {
  let result = recover {
    let x = foo();
    x + 1
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn recover_block_empty() {
    let input = r#"
fn test() {
  let result = recover {};
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn recover_block_with_semicolon() {
    let input = r#"
fn test() {
  let result = recover {
    do_something();
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn recover_block_nested() {
    let input = r#"
fn test() {
  let outer = recover {
    let inner = recover {
      inner_risky()
    };
    outer_risky()
  };
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn negative_literal_on_new_line_is_unary() {
    let input = r#"
fn test() -> int {
  let x = {
    foo()
    -1
  };
  x
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn negative_pattern_integer() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    -5 => "negative five",
    -1 => "negative one",
    0 => "zero",
    _ => "other",
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn nested_tuple_field_access() {
    let input = r#"
fn test(nested: ((int, int), int)) {
  let x = nested.0.0;
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn enum_variant_rest_pattern() {
    let input = r#"
enum Shape { Triangle(int, int, int), Circle(float) }
fn test(s: Shape) {
  match s {
    Shape.Triangle(..) => print("triangle"),
    Shape.Circle(..) => print("circle"),
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn format_string_with_tuple_on_next_line() {
    let input = r#"
fn test1() -> (int, string) {
  let a = 10
  let b = f"hello {a}"
  (a, b)
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn format_string_with_slice_literal_on_next_line() {
    let input = r#"
fn test() -> Slice<string> {
  let name = "test"
  let msg = f"items for {name}"
  [msg, msg + "2"]
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_spread_arg() {
    let input = r#"
fn test() { func(..xs); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_leading_args_and_spread_arg() {
    let input = r#"
fn test() { func(a, b, ..xs); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_spread_arg_trailing_comma() {
    let input = r#"
fn test() { func(..xs,); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn call_with_leading_args_and_spread_arg_trailing_comma() {
    let input = r#"
fn test() { func(a, b, ..xs,); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn raw_string_literal_assignment() {
    let input = r#"
fn test() { let x = r"\d+\.\d+"; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn raw_string_in_function_arg() {
    let input = r#"
fn test() { f(r"C:\Users\me"); }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn raw_string_in_slice() {
    let input = r#"
fn test() { let xs = [r"\d", r"\s", "plain"]; }
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn raw_string_in_match_pattern() {
    let input = r#"
fn test() {
  match s {
    r"\d+" => 1,
    "plain" => 2,
    _ => 0,
  }
}
"#;
    assert_parse_snapshot!(input);
}

#[test]
fn raw_string_in_rawgo_directive() {
    let input = r#"
fn test() { @rawgo(r"if x > 0 {\n}") }
"#;
    assert_parse_snapshot!(input);
}
