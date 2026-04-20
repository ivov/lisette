use crate::{assert_lint_snapshot, assert_no_lint_warnings};

#[test]
fn unused_variable() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  ()
}
"#
    );
}

#[test]
fn unused_as_alias_in_match_arm() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let opt: Option<int> = Some(1)
  match opt {
    Some(n) as unused => n,
    None => 0,
  };
}
"#
    );
}

#[test]
fn unused_variable_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _x = 5;
  ()
}
"#
    );
}

#[test]
fn unused_variable_struct_field_shorthand() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int }

fn main() {
  let p = Point { x: 1 };
  let Point { x } = p;
  ()
}
"#
    );
}

#[test]
fn unused_variable_struct_field_explicit() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int }

fn main() {
  let p = Point { x: 1 };
  let Point { x: foo } = p;
  ()
}
"#
    );
}

#[test]
fn used_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  x
}
"#
    );
}

#[test]
fn or_pattern_binding_no_spurious_unused_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let t = (42, 1);
  let _ = match t {
    (x, 1) | (x, 2) => x,
    _ => 0,
  };
}
"#
    );
}

#[test]
fn unused_mut() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x
}
"#
    );
}

#[test]
fn used_mut_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 5;
  x = 10;
  x
}
"#
    );
}

#[test]
fn mut_param_no_unnecessary_mut_warning() {
    assert_no_lint_warnings!(
        r#"
fn process(mut items: Slice<int>) -> Slice<int> {
  items = items.append(42);
  items
}

fn main() {
  let mut x = [3, 1, 2];
  let _ = process(x)
}
"#
    );
}

#[test]
fn referenced_mut_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn mutate(r: Ref<int>) {
  r.* = 99
}

fn main() {
  let mut x = 5;
  mutate(&x);
  x
}
"#
    );
}

#[test]
fn ref_method_mut_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Counter {
  value: int,
}

impl Counter {
  fn increment(self: Ref<Counter>) {
    self.value += 1;
  }

  fn get(self: Counter) -> int {
    self.value
  }
}

fn main() {
  let mut c = Counter { value: 0 };
  c.increment();
  c.get()
}
"#
    );
}

#[test]
fn written_but_not_read_simple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 0
  x = 42
  ()
}
"#
    );
}

#[test]
fn written_but_not_read_simple_reassignment() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut a = 0
  a = 42
  ()
}
"#
    );
}

#[test]
fn written_but_not_read_in_match() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut status = "init"
  let opt: Option<int> = None
  match opt {
    Some(_) => { status = "found" },
    None => { status = "missing" },
  }
  ()
}
"#
    );
}

#[test]
fn written_but_not_read_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut _flag = false
  _flag = true
  ()
}
"#
    );
}

#[test]
fn written_and_read_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 0
  x = 42
  x
}
"#
    );
}

#[test]
fn unused_value() {
    assert_lint_snapshot!(
        r#"
fn main() {
  1 + 2;
  ()
}
"#
    );
}

#[test]
fn unused_literal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  42;
  ()
}
"#
    );
}

#[test]
fn unused_result() {
    assert_lint_snapshot!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  get_result();
  ()
}
"#
    );
}

#[test]
fn unused_result_handled_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  let _ = get_result();
  ()
}
"#
    );
}

#[test]
fn allow_unused_result_suppresses_lint() {
    assert_no_lint_warnings!(
        r#"
#[allow(unused_result)]
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  get_result();
  ()
}
"#
    );
}

#[test]
fn allow_unused_result_scoped_to_annotated_function() {
    assert_lint_snapshot!(
        r#"
#[allow(unused_result)]
fn safe_call() -> Result<int, string> {
  Ok(1)
}

fn unsafe_call() -> Result<int, string> {
  Ok(2)
}

fn main() {
  safe_call();
  unsafe_call();
  ()
}
"#
    );
}

#[test]
fn allow_unused_result_does_not_suppress_option() {
    assert_lint_snapshot!(
        r#"
#[allow(unused_result)]
fn get_option() -> Option<int> {
  Some(42)
}

fn main() {
  get_option();
  ()
}
"#
    );
}

#[test]
fn unused_option() {
    assert_lint_snapshot!(
        r#"
fn get_option() -> Option<int> {
  Some(42)
}

fn main() {
  get_option();
  ()
}
"#
    );
}

#[test]
fn unused_option_handled_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_option() -> Option<int> {
  Some(42)
}

fn main() {
  let _ = get_option();
  ()
}
"#
    );
}

#[test]
fn match_in_statement_position_no_unused_result_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  let r = get_result();
  match r {
    Ok(_) => "ok",
    Err(_) => "err",
  }
  ()
}
"#
    );
}

#[test]
fn match_in_statement_position_no_unused_value_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 1;
  match x {
    1 => "one",
    _ => "other",
  }
  ()
}
"#
    );
}

#[test]
fn if_in_statement_position_no_unused_value_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 1;
  if x > 0 {
    "positive"
  } else {
    "negative"
  }
  ()
}
"#
    );
}

#[test]
fn unused_param() {
    assert_lint_snapshot!(
        r#"
pub fn foo(x: int) -> int {
  42
}
"#
    );
}

#[test]
fn unused_param_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(_x: int) -> int {
  42
}
"#
    );
}

#[test]
fn used_param_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(x: int) -> int {
  x
}
"#
    );
}

#[test]
fn self_assignment() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = x;
  x
}
"#
    );
}

#[test]
fn self_assignment_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = (x);
  x
}
"#
    );
}

#[test]
fn self_comparison_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  x == x
}
"#
    );
}

#[test]
fn self_comparison_less_than() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  x < x
}
"#
    );
}

#[test]
fn self_comparison_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  (x) == x
}
"#
    );
}

#[test]
fn self_comparison_float_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: float64 = 0.0;
  x == x
}
"#
    );
}

#[test]
fn double_bool_negation() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  !!x
}
"#
    );
}

#[test]
fn double_int_negation() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  --x
}
"#
    );
}

#[test]
fn double_bool_negation_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  !(!x)
}
"#
    );
}

#[test]
fn unused_function() {
    assert_lint_snapshot!(
        r#"
fn unused_helper() -> int {
  42
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn helper() -> int {
  42
}

fn main() {
  helper()
}
"#
    );
}

#[test]
fn unused_struct() {
    assert_lint_snapshot!(
        r#"
struct UnusedPoint {
  x: int,
  y: int,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_struct_all_fields_read_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

fn main() {
  let p = Point { x: 1, y: 2 };
  p.x + p.y
}
"#
    );
}

#[test]
fn unused_enum() {
    assert_lint_snapshot!(
        r#"
enum UnusedColor {
  Red,
  Green,
  Blue,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_enum_all_variants_used_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Color {
  Red,
  Green,
}

fn main() {
  let c = Color.Red;
  match c {
    Color.Red => 1,
    Color.Green => 2,
  }
}
"#
    );
}

#[test]
fn unused_constant() {
    assert_lint_snapshot!(
        r#"
const UNUSED_VALUE = 42

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
const VALUE = 42

fn main() {
  VALUE
}
"#
    );
}

#[test]
fn public_function_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub fn public_helper() -> int {
  42
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn public_struct_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub struct PublicPoint {
  x: int,
  y: int,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn function_reachable_through_chain() {
    assert_no_lint_warnings!(
        r#"
fn helper1() -> int {
  42
}

fn helper2() -> int {
  helper1()
}

fn main() {
  helper2()
}
"#
    );
}

#[test]
fn struct_used_in_signature() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

fn create_point() -> Point {
  Point { x: 0, y: 0 }
}

fn main() {
  create_point()
}
"#
    );
}

#[test]
fn struct_used_in_parameter() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

fn get_x(p: Point) -> int {
  p.x
}

fn main() {
  get_x(Point { x: 1, y: 2 })
}
"#
    );
}

#[test]
fn internal_type_leak() {
    assert_lint_snapshot!(
        r#"
struct PrivateData {
  _value: int,
}

pub fn leaky_function() -> PrivateData {
  PrivateData { _value: 42 }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn public_type_in_public_signature_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct PublicData {
  value: int,
}

pub fn public_function() -> PublicData {
  PublicData { value: 42 }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn internal_type_leak_in_tuple_return() {
    assert_lint_snapshot!(
        r#"
struct PrivateA {
  _a: int,
}

struct PrivateB {
  _b: int,
}

pub fn get_pair() -> (PrivateA, PrivateB) {
  (PrivateA { _a: 1 }, PrivateB { _b: 2 })
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn internal_type_leak_in_higher_order_function() {
    assert_lint_snapshot!(
        r#"
struct PrivateOutput {
  _result: int,
}

pub fn make_handler(seed: int) -> fn() -> PrivateOutput {
  || PrivateOutput { _result: seed }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn unused_import() {
    assert_lint_snapshot!(
        r#"
import "some/module"

fn main() {
  ()
}
"#
    );
}

#[test]
fn enum_struct_variant_constructor_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Shape {
  Circle { radius: float64 },
}

fn make_circle() -> Shape {
    Shape.Circle { radius: 5.0 }
}

fn main() {
    let _s = make_circle()
}
"#
    );
}

#[test]
fn unused_struct_field() {
    assert_lint_snapshot!(
        r#"
struct Data {
  used_field: int,
  unused_field: int,
}

fn make_data() -> Data {
  Data { used_field: 1, unused_field: 2 }
}

fn main() {
  let d = make_data();
  d.used_field
}
"#
    );
}

#[test]
fn used_struct_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

fn main() {
  let p = Point { x: 1, y: 2 };
  p.x + p.y
}
"#
    );
}

#[test]
fn public_struct_fields_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn serialization_struct_fields_not_unused() {
    assert_no_lint_warnings!(
        r#"
#[json]
struct Response {
  status: string,
  code: int,
}

fn main() {
  let _r = Response { status: "ok", code: 200 }
}
"#
    );
}

#[test]
fn struct_field_used_in_pattern() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

fn main() {
  let p = Point { x: 1, y: 2 };
  match p {
    Point { x, y } => x + y,
  }
}
"#
    );
}

#[test]
fn struct_field_used_in_match_subject() {
    assert_no_lint_warnings!(
        r#"
struct Container {
  value: int,
}

fn main() {
  let c = Container { value: 42 };
  match c.value {
    _ => 0,
  }
}
"#
    );
}

#[test]
fn struct_field_with_option_used_in_match_subject() {
    assert_no_lint_warnings!(
        r#"
struct Container {
  value: Option<int>,
}

fn main() {
  let c = Container { value: Some(42) };
  match c.value {
    Some(n) => n,
    None => 0,
  }
}
"#
    );
}

#[test]
fn struct_field_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
struct Data {
  used: int,
  _unused: int,
}

fn main() {
  let d = Data { used: 1, _unused: 2 };
  d.used
}
"#
    );
}

#[test]
fn struct_field_used_via_spread_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Config {
  debug: bool,
  verbose: bool,
  port: int,
}

fn main() {
  let base = Config { debug: false, verbose: false, port: 8080 };
  let dev = Config { debug: true, ..base };
  if dev.debug { dev.port } else { 0 }
}
"#
    );
}

#[test]
fn struct_field_used_via_type_alias_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

type MyPoint = Point

fn read(p: MyPoint) -> int {
  p.x + p.y
}

fn main() {
  read(Point { x: 1, y: 2 })
}
"#
    );
}

#[test]
fn unused_enum_variant() {
    assert_lint_snapshot!(
        r#"
enum Color {
  Red,
  Unused,
}

fn main() {
  Color.Red
}
"#
    );
}

#[test]
fn used_enum_variant_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Color {
  Red,
  Green,
}

fn main() {
  let c = Color.Red;
  match c {
    Color.Red => 1,
    Color.Green => 2,
  }
}
"#
    );
}

#[test]
fn public_enum_variants_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub enum Color {
  Red,
  Green,
  Blue,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn enum_variant_used_in_pattern() {
    assert_no_lint_warnings!(
        r#"
enum Status {
  Active(int),
  Inactive,
}

fn main() {
  let s = Status.Active(42);
  match s {
    Status.Active(x) => x,
    Status.Inactive => 0,
  }
}
"#
    );
}

#[test]
fn enum_variant_used_in_pattern_unqualified() {
    assert_no_lint_warnings!(
        r#"
enum Color {
  Red,
  Green,
  Blue,
}

fn main() {
  let c = Color.Blue;
  match c {
    Red => 1,
    Green => 2,
    Blue => 3,
  }
}
"#
    );
}

#[test]
fn match_on_literal_slice() {
    assert_lint_snapshot!(
        r#"
fn main() {
  match [1, 2, 3] {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_literal_tuple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  match (1, 2) {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_nested_paren_literal_tuple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  match (((1, 2))) {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3];
  match xs {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_tuple_of_variables_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = Some(1);
  let b = Some(2);
  match (a, b) {
    (Some(x), Some(y)) => x + y,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn dead_code_after_return() {
    assert_lint_snapshot!(
        r#"
pub fn foo() -> int {
  return 42;
  let x = 1;
  x
}
"#
    );
}

#[test]
fn no_dead_code_when_return_is_last() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() -> int {
  return 42
}
"#
    );
}

#[test]
fn dead_code_after_break() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    break;
    let x = 1;
    x
  }
}
"#
    );
}

#[test]
fn no_dead_code_when_break_is_last() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    break
  }
}
"#
    );
}

#[test]
fn dead_code_after_continue() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    continue;
    let x = 1;
    x
  }
}
"#
    );
}

#[test]
fn no_dead_code_when_continue_is_last() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    continue
  }
}
"#
    );
}

#[test]
fn dead_code_after_diverging_if_else() {
    assert_lint_snapshot!(
        r#"
pub fn foo() -> int {
  if true {
    return 1
  } else {
    return 2
  };
  let x = 3;
  x
}
"#
    );
}

#[test]
fn no_dead_code_when_only_one_branch_returns() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() -> int {
  if true {
    return 1
  } else {
    2
  }
}
"#
    );
}

#[test]
fn dead_code_after_diverging_match() {
    assert_lint_snapshot!(
        r#"
pub fn foo(x: int) -> int {
  match x {
    0 => return 0,
    _ => return 1,
  };
  let y = 2;
  y
}
"#
    );
}

#[test]
fn no_dead_code_when_not_all_match_arms_diverge() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(x: int) -> int {
  match x {
    0 => return 0,
    _ => 1,
  }
}
"#
    );
}

#[test]
fn dead_code_after_diverging_nested_block() {
    assert_lint_snapshot!(
        r#"
pub fn foo() -> int {
  { return 1 };
  let x = 2;
  x
}
"#
    );
}

#[test]
fn no_dead_code_after_loop_with_break() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() -> int {
  loop { break };
  42
}
"#
    );
}

#[test]
fn no_dead_code_after_while_with_break() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() -> int {
  while true { break };
  42
}
"#
    );
}

#[test]
fn no_dead_code_after_closure_with_return() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() -> int {
  let _f = || { return 1 };
  42
}
"#
    );
}

#[test]
fn no_dead_code_when_if_has_no_else() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(cond: bool) -> int {
  if cond { return 1 };
  42
}
"#
    );
}

#[test]
fn dead_code_after_infinite_loop() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    ()
  };
  let x = 1;
  x
}
"#
    );
}

#[test]
fn no_dead_code_after_loop_with_conditional_break() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    if true {
      break
    } else {
      ()
    }
  };
  ()
}
"#
    );
}

#[test]
fn no_dead_code_after_loop_with_nested_break() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    match 1 {
      1 => break,
      _ => (),
    }
  };
  ()
}
"#
    );
}

#[test]
fn dead_code_after_diverging_call() {
    assert_lint_snapshot!(
        r#"
fn diverge() -> Never {
  loop { () }
}

fn main() {
  diverge();
  let x = 1;
  x
}
"#
    );
}

#[test]
fn no_dead_code_after_normal_call() {
    assert_no_lint_warnings!(
        r#"
pub fn normal() {
  ()
}

fn main() {
  normal();
  ()
}
"#
    );
}

#[test]
fn interface_references_used_type() {
    assert_no_lint_warnings!(
        r#"
pub struct Data {
  value: int,
}

pub interface Container {
  fn get() -> Data;
  fn set(_d: Data);
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn unused_interface_warning() {
    assert_lint_snapshot!(
        r#"
interface Processor {
  fn process(_x: int);
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_used_in_embedding_not_unused() {
    assert_no_lint_warnings!(
        r#"
interface HasName {
  fn name(self) -> string
}

pub interface Person {
  impl HasName
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_self_param_no_unused_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Greetable {
  fn greet(self) -> string;
  fn update(self, value: int);
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn for_loop_uses_function() {
    assert_no_lint_warnings!(
        r#"
fn get_items() -> Slice<int> {
  [1, 2, 3]
}

fn main() {
  for item in get_items() {
    let _x = item + 1;
  }
}
"#
    );
}

#[test]
fn for_loop_uses_struct() {
    assert_no_lint_warnings!(
        r#"
struct Item {
  value: int,
}

fn main() {
  let items = [Item { value: 1 }, Item { value: 2 }];
  for item in items {
    let _x = item.value;
  }
}
"#
    );
}

#[test]
fn select_uses_channel_type() {
    assert_no_lint_warnings!(
        r#"
pub struct Data {
  value: int,
}

fn main() {
  let ch = Channel.new<Data>();
  select {
    let Some(d) = ch.receive() => d.value,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn type_alias_uses_struct() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

pub type Location = Point

fn main() {
  let loc: Location = Point { x: 1, y: 2 };
  loc.x
}
"#
    );
}

#[test]
fn unused_type_alias_warning() {
    assert_lint_snapshot!(
        r#"
type Unused = int

fn main() {
  ()
}
"#
    );
}

#[test]
fn type_alias_used_in_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Ints = Slice<int>

fn sum(nums: Ints) -> int {
  let mut total = 0;
  for n in nums {
    total += n;
  }
  total
}

fn main() {
  sum([1, 2, 3])
}
"#
    );
}

#[test]
fn type_alias_used_in_let_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Ints = Slice<int>

fn main() {
  let nums: Ints = [1, 2, 3];
  nums[0]
}
"#
    );
}

#[test]
fn type_alias_used_in_another_type_alias_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Inner = Option<int>
type Outer = Option<Inner>

fn unwrap_nested(o: Outer) -> int {
  match o {
    Some(Some(x)) => x,
    Some(None) => -1,
    None => -2,
  }
}

fn main() {
  let x: Outer = Some(Some(42));
  unwrap_nested(x)
}
"#
    );
}

#[test]
fn type_alias_used_in_struct_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
type UserId = int

struct User {
  id: UserId,
  name: string,
}

fn main() {
  let u = User { id: 1, name: "Alice" }
  u.id + u.name.len() as int
}
"#
    );
}

#[test]
fn type_alias_used_in_const_annotation_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Limit = int

const MAX: Limit = 100

fn main() {
  MAX + 1
}
"#
    );
}

#[test]
fn type_alias_used_in_cast_expression_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Score = float64

fn main() {
  let x = 42 as Score
  x + 1.0
}
"#
    );
}

#[test]
fn type_used_via_static_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Vec2 { x: int, y: int }

impl Vec2 {
  fn new(x: int, y: int) -> Vec2 {
    Vec2 { x: x, y: y }
  }

  fn length_squared(self: Vec2) -> int {
    self.x * self.x + self.y * self.y
  }
}

fn main() {
  let v1 = Vec2.new(3, 4);
  v1.length_squared()
}
"#
    );
}

#[test]
fn format_string_uses_variable() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let name = "world";
  let msg = f"Hello, {name}!";
  msg
}
"#
    );
}

#[test]
fn uninterpolated_fstring() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let msg = f"hello world";
  msg
}
"#
    );
}

#[test]
fn expression_only_fstring() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let name = "world";
  let msg = f"{name}";
  msg
}
"#
    );
}

#[test]
fn fstring_with_text_and_interpolation_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let name = "world";
  let msg = f"hello {name}";
  msg
}
"#
    );
}

#[test]
fn fstring_with_non_string_expression_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let num = 42;
  let msg = f"{num}";
  msg
}
"#
    );
}

#[test]
fn slice_literal_uses_function() {
    assert_no_lint_warnings!(
        r#"
fn one() -> int { 1 }
fn two() -> int { 2 }

fn main() {
  let xs = [one(), two(), 3];
  xs[0]
}
"#
    );
}

#[test]
fn propagate_expression_uses_result_type() {
    assert_no_lint_warnings!(
        r#"
pub struct Error {
  message: string,
}

fn might_fail() -> Result<int, Error> {
  Ok(42)
}

fn run() -> Result<int, Error> {
  let value = might_fail()?;
  Ok(value)
}

fn main() { let _ = run() }
"#
    );
}

#[test]
fn tuple_uses_struct_types() {
    assert_no_lint_warnings!(
        r#"
pub struct First { a: int }
pub struct Second { b: string }

fn main() {
  let pair = (First { a: 1 }, Second { b: "x" });
  let (first, _second) = pair;
  first.a + 1
}
"#
    );
}

#[test]
fn paren_expression_uses_function() {
    assert_no_lint_warnings!(
        r#"
fn compute() -> int { 42 }

fn main() {
  let result = (compute()) + 1;
  result
}
"#
    );
}

#[test]
fn reference_expression_uses_variable() {
    assert_no_lint_warnings!(
        r#"
pub struct Data { value: int }

fn main() {
  let d = Data { value: 42 };
  let ptr = &d;
  ptr.value
}
"#
    );
}

#[test]
fn division_by_non_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test() {
  let x = 10 / 2;
  x
}
"#
    );
}

#[test]
fn empty_match_arm() {
    assert_lint_snapshot!(
        r#"
pub fn test() {
  let opt: Option<int> = None;
  match opt {
    Some(_x) => {},
    None => (),
  }
}
"#
    );
}

#[test]
fn match_arm_with_unit_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test() {
  let opt: Option<int> = None;
  match opt {
    Some(_) => (),
    None => (),
  }
}
"#
    );
}

#[test]
fn unnecessary_reference() {
    assert_lint_snapshot!(
        r#"
pub fn foo(x: Ref<int>) {
  let _ = &x;
}
"#
    );
}

#[test]
fn necessary_reference_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 42;
  let _ = &x;
}
"#
    );
}

#[test]
fn unused_type_parameter() {
    assert_lint_snapshot!(
        r#"
pub fn process<T>(x: int) -> int {
  x
}
"#
    );
}

#[test]
fn used_type_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn identity<T>(x: T) -> T {
  x
}

fn main() {
  let _ = identity(42);
}
"#
    );
}

#[test]
fn type_param_only_in_bound_warns() {
    assert_lint_snapshot!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

pub fn squiggle<A: Cloner<B>, B>(_: A) {}
"#
    );
}

#[test]
fn type_param_in_bound_and_used_as_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

struct Foo{}

impl Foo {
  fn clone(self) -> Foo { Foo{} }
}

pub fn squiggle<A: Cloner<B>, B>(_: A, _: B) {}

fn main() {
  squiggle(Foo{}, Foo{})
}
"#
    );
}

#[test]
fn type_param_in_bound_and_used_as_return_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

struct Foo{}

impl Foo {
  fn clone(self) -> Foo { Foo{} }
}

pub fn squiggle<A: Cloner<B>, B>(a: A) -> B {
  a.clone()
}
"#
    );
}

#[test]
fn type_param_only_in_bound_underscore_prefix_suppressed() {
    assert_no_lint_warnings!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

pub fn squiggle<A: Cloner<_B>, _B>(_: A) {}
"#
    );
}

#[test]
fn interface_used_as_struct_type_parameter_constraint_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

interface Showable {
  fn show(self) -> string
}

struct Wrapper<T: Showable> {
  inner: T,
}

impl<T: Showable> Wrapper<T> {
  fn display(self) -> string {
    self.inner.show()
  }
}

struct Name {
  value: string,
}

impl Name {
  fn show(self) -> string {
    self.value
  }
}

fn main() {
  let w = Wrapper { inner: Name { value: "test" } }
  fmt.Println(w.display())
}
"#
    );
}

#[test]
fn rest_only_slice_pattern_discard() {
    assert_lint_snapshot!(
        r#"
pub fn test(slice: Slice<int>) {
  let [..] = slice;
}
"#
    );
}

#[test]
fn rest_only_slice_pattern_bind() {
    assert_lint_snapshot!(
        r#"
pub fn test(slice: Slice<int>) {
  let [..rest] = slice;
  rest
}
"#
    );
}

#[test]
fn non_pascal_case_struct() {
    assert_lint_snapshot!(
        r#"
struct point { x: int, y: int }

fn main() {
  let _ = point { x: 1, y: 2 };
}
"#
    );
}

#[test]
fn non_pascal_case_enum() {
    assert_lint_snapshot!(
        r#"
enum color { Red, Green, Blue }

fn main() {}
"#
    );
}

#[test]
fn pascal_case_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

struct Point { x: int, y: int }

fn main() {
  let p = Point { x: 1, y: 2 };
  fmt.Print(p.x + p.y);
}
"#
    );
}

#[test]
fn non_snake_case_function() {
    assert_lint_snapshot!(
        r#"
fn getUserId() -> int { 42 }

fn main() {
  let _ = getUserId();
}
"#
    );
}

#[test]
fn snake_case_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_user_id() -> int { 42 }

fn main() {
  let _ = get_user_id();
}
"#
    );
}

#[test]
fn non_snake_case_variable() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let userId = 42;
  let _ = userId;
}
"#
    );
}

#[test]
fn snake_case_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let user_id = 42;
  let _ = user_id;
}
"#
    );
}

#[test]
fn non_snake_case_parameter() {
    assert_lint_snapshot!(
        r#"
fn greet(userId: int) {
  let _ = userId;
}

fn main() {
  greet(42);
}
"#
    );
}

#[test]
fn snake_case_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn greet(user_id: int) {
  let _ = user_id;
}

fn main() {
  greet(42);
}
"#
    );
}

#[test]
fn non_snake_case_struct_field() {
    assert_lint_snapshot!(
        r#"
struct User { oddsAndEnds: int }

fn main() {
  let u = User { oddsAndEnds: 42 };
  let _ = u.oddsAndEnds;
}
"#
    );
}

#[test]
fn snake_case_struct_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct User { odds_and_ends: int }

fn main() {
  let u = User { odds_and_ends: 42 };
  let _ = u.odds_and_ends;
}
"#
    );
}

#[test]
fn non_screaming_snake_case_constant() {
    assert_lint_snapshot!(
        r#"
const maxRetries = 3;

fn main() {
  let _ = maxRetries;
}
"#
    );
}

#[test]
fn screaming_snake_case_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
const MAX_RETRIES = 3;

fn main() {
  let _ = MAX_RETRIES;
}
"#
    );
}

#[test]
fn underscore_prefix_suppresses_casing_warnings() {
    assert_no_lint_warnings!(
        r#"
fn _getUserId() -> int { 42 }

fn main() {
  let _ = _getUserId();
}
"#
    );
}

#[test]
fn non_pascal_case_type_parameter() {
    assert_lint_snapshot!(
        r#"
fn identity<t>(x: t) -> t { x }

fn main() {
  let _ = identity(42);
}
"#
    );
}

#[test]
fn pascal_case_type_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn identity<T>(x: T) -> T { x }

fn main() {
  let _ = identity(42);
}
"#
    );
}

#[test]
fn non_pascal_case_enum_variant() {
    assert_lint_snapshot!(
        r#"
pub enum Status { pending, completed }

fn main() {
  let _ = Status.pending;
}
"#
    );
}

#[test]
fn pascal_case_enum_variant_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Status { Pending, Completed }

fn main() {
  let _ = Status.Pending;
  let _ = Status.Completed;
}
"#
    );
}

#[test]
fn irrefutable_if_let_identifier() {
    assert_lint_snapshot!(
        r#"
pub fn test(x: int) {
  if let y = x {
    let _ = y;
  }
}
"#
    );
}

#[test]
fn irrefutable_if_let_tuple() {
    assert_lint_snapshot!(
        r#"
pub fn test(pair: (int, int)) {
  if let (a, b) = pair {
    let _ = a + b;
  }
}
"#
    );
}

#[test]
fn refutable_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn single_arm_match_option() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    Some(x) => { let _ = x; },
    _ => (),
  }
}
"#
    );
}

#[test]
fn single_arm_match_result() {
    assert_lint_snapshot!(
        r#"
pub fn test(res: Result<int, string>) {
  match res {
    Ok(x) => { let _ = x; },
    _ => (),
  }
}
"#
    );
}

#[test]
fn multi_arm_match_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(opt: Option<int>) {
  match opt {
    Some(x) => { let _ = x; },
    None => { let _ = 0; },
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn redundant_if_let_else() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  } else {
  }
}
"#
    );
}

#[test]
fn redundant_if_let_else_unit() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  } else {
    ()
  }
}
"#
    );
}

#[test]
fn if_let_with_meaningful_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  } else {
    let _ = 0;
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn irrefutable_if_let_struct() {
    assert_lint_snapshot!(
        r#"
pub struct Point { x: int, y: int }

pub fn test(p: Point) {
  if let Point { x, y } = p {
    let _ = x + y;
  }
}
"#
    );
}

#[test]
fn redundant_let_else() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  let x = opt else { return; };
  let _ = x;
}
"#
    );
}

#[test]
fn enum_variant_used_in_while_let_pattern() {
    assert_no_lint_warnings!(
        r#"
enum Status {
  Active(int),
  Done,
}

fn main() {
  let mut s = Status.Active(3);
  while let Status.Active(x) = s {
    if x <= 1 {
      s = Status.Done
    } else {
      s = Status.Active(x - 1)
    };
    s
  }
}
"#
    );
}

#[test]
fn refutable_or_pattern_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum E { A, B, C }

fn test(e: E) {
  if let A | B = e {
    ();
  }
}

fn main() {
  test(E.A);
  test(E.B);
  test(E.C);
}
"#
    );
}

#[test]
fn irrefutable_or_pattern_if_let_warning() {
    assert_lint_snapshot!(
        r#"
fn test(opt: Option<int>) {
  if let Some(_) | None = opt {
    ();
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn try_block_no_success_path_err() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let result: Result<int, string> = try {
    Err("fail")?
  };
  let _ = result;
}
"#
    );
}

#[test]
fn try_block_no_success_path_none() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let result: Option<int> = try {
    None?
  };
  let _ = result;
}
"#
    );
}

#[test]
fn excess_parens_on_condition_if() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  if (x > 0) {
    let _ = x;
  }
}
"#
    );
}

#[test]
fn excess_parens_on_condition_while() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut i = 0;
  while (i < 10) {
    i = i + 1;
  }
  let _ = i;
}
"#
    );
}

#[test]
fn excess_parens_on_condition_match() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = match (x) {
    0 => 0,
    _ => 1,
  };
}
"#
    );
}

#[test]
fn unknown_attribute() {
    assert_lint_snapshot!(
        r#"
#[foo]
pub struct User {
  name: string,
}
"#
    );
}

#[test]
fn field_attribute_without_struct_attribute() {
    assert_lint_snapshot!(
        r#"
pub struct User {
  #[json(omitempty)]
  name: string,
}
"#
    );
}

#[test]
fn duplicate_tag_key() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct User {
  #[json(omitempty)]
  #[json(skip)]
  name: string,
}
"#
    );
}

#[test]
fn conflicting_case_transforms() {
    assert_lint_snapshot!(
        r#"
#[json(snake_case, camel_case)]
pub struct User {
  first_name: string,
}
"#
    );
}

#[test]
fn raw_tags_different_keys_no_duplicate() {
    assert_no_lint_warnings!(
        r#"
#[tag("validate")]
#[tag("custom")]
pub struct User {
  #[tag(`validate:"required"`)]
  #[tag(`custom:"foo"`)]
  name: string,
}
"#
    );
}

#[test]
fn raw_tag_plus_alias_same_key_duplicate() {
    assert_lint_snapshot!(
        r#"
#[tag("validate")]
pub struct User {
  #[tag(`validate:"required"`)]
  #[tag("validate", "email")]
  name: string,
}
"#
    );
}

#[test]
fn raw_tag_should_use_alias() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct User {
  #[tag(`json:"user_name"`)]
  name: string,
}
"#
    );
}

#[test]
fn struct_tag_satisfies_field_alias() {
    assert_no_lint_warnings!(
        r#"
#[tag("json")]
pub struct User {
  #[json("name")]
  name: string,
}
"#
    );
}

#[test]
fn field_tag_requires_struct_opt_in() {
    assert_lint_snapshot!(
        r#"
pub struct User {
  #[bson("custom_name")]
  name: string,
}
"#
    );
}

#[test]
fn unknown_tag_option_warns() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct User {
  #[json(unknown_flag)]
  name: string,
}
"#
    );
}

#[test]
fn known_tag_options_no_warning() {
    assert_no_lint_warnings!(
        r#"
#[json(snake_case, omitempty)]
pub struct User {
  #[json("user_name", omitempty, skip)]
  name: string,
  #[json(camel_case, string)]
  age: int,
  #[json(!omitempty)]
  active: bool,
}
"#
    );
}

#[test]
fn struct_fields_accessed_through_ref_not_unused() {
    assert_no_lint_warnings!(
        r#"
struct Node {
  value: int,
  next: Option<Ref<Node>>,
}

fn sum_list(node: Option<Ref<Node>>) -> int {
  match node {
    None => 0,
    Some(n) => n.value + sum_list(n.next),
  }
}

fn main() -> int {
  let c = Node { value: 3, next: None }
  let b = Node { value: 2, next: Some(&c) }
  let a = Node { value: 1, next: Some(&b) }
  sum_list(Some(&a))
}
"#
    );
}

#[test]
fn interface_used_as_generic_bound_not_unused() {
    assert_no_lint_warnings!(
        r#"
interface Describable {
  fn describe(self) -> string
}

struct Dog {
  name: string,
}

impl Dog {
  fn describe(self: Dog) -> string {
    self.name
  }
}

fn print_thing<T: Describable>(thing: T) -> string {
  thing.describe()
}

fn main() -> string {
  print_thing(Dog { name: "Rex" })
}
"#
    );
}

#[test]
fn interface_method_via_structural_typing_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Describable {
  fn describe(self) -> string
}

pub fn print_desc(d: Describable) -> string {
  d.describe()
}

struct Dog {
  name: string,
}

impl Dog {
  fn describe(self) -> string {
    f"Dog: {self.name}"
  }
}

fn main() {
  let d = Dog { name: "Rex" }
  print_desc(d)
}
"#
    );
}

#[test]
fn interface_method_unused_still_warns() {
    assert_lint_snapshot!(
        r#"
pub interface Describable {
  fn describe(self) -> string
}

struct Dog {
  name: string,
}

impl Dog {
  fn describe(self) -> string {
    f"Dog: {self.name}"
  }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_method_multiple_implementers_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Animal {
  fn speak(self) -> string
}

fn make_sound(a: Animal) -> string {
  a.speak()
}

struct Dog {
  name: string,
}

impl Dog {
  fn speak(self) -> string {
    self.name
  }
}

struct Cat {
  name: string,
}

impl Cat {
  fn speak(self) -> string {
    self.name
  }
}

fn main() {
  let dog = Dog { name: "Rex" }
  let cat = Cat { name: "Whiskers" }
  let _ = make_sound(dog)
  let _ = make_sound(cat)
}
"#
    );
}

#[test]
fn impl_method_pascal_case_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct MyError { message: string }

impl MyError {
  fn Error(self) -> string {
    self.message
  }
}

fn main() {
  let e = MyError { message: "fail" };
  let _ = e.Error();
}
"#
    );
}

#[test]
fn standalone_function_pascal_case_still_warns() {
    assert_lint_snapshot!(
        r#"
fn GetUserId() -> int { 42 }

fn main() {
  let _ = GetUserId();
}
"#
    );
}

#[test]
fn unused_self_in_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Circle { radius: float64 }

impl Circle {
  fn name(self) -> string {
    "circle"
  }
}

fn main() {
  let c = Circle { radius: 1.0 };
  let _ = c.name();
}
"#
    );
}

#[test]
fn interface_used_as_struct_field_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Greeter {
  fn greet(self) -> string
}

struct Person { name: string }
impl Person {
  fn greet(self) -> string { self.name }
}

struct App {
  greeter: Greeter,
}

fn main() {
  let app = App { greeter: Person { name: "Alice" } };
  let _ = app.greeter.greet();
}
"#
    );
}

#[test]
fn interface_used_as_enum_variant_field_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Handler {
  fn handle(self) -> string
}

struct MyHandler {}
impl MyHandler {
  fn handle(self) -> string { "ok" }
}

enum Action {
  Run { handler: Handler },
  Skip,
}

fn main() -> string {
  let action = Action.Run { handler: MyHandler {} };
  match action {
    Action.Run { handler } => handler.handle(),
    Action.Skip => "skipped",
  }
}
"#
    );
}

#[test]
fn type_used_in_turbofish_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Worker {
  fn work(self) -> string;
}

struct Greeter { name: string }

impl Greeter {
  fn work(self) -> string { self.name }
}

fn main() {
  let ch = Channel.new<Worker>();
  ch.send(Greeter { name: "test" });
  ch.close()
}
"#
    );
}

#[test]
fn unused_result_in_tail_position() {
    assert_lint_snapshot!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn do_work() {
  get_result()
}

fn main() {
  do_work()
}
"#
    );
}

#[test]
fn unused_option_in_tail_position() {
    assert_lint_snapshot!(
        r#"
fn find_item() -> Option<int> {
  Some(42)
}

fn do_search() {
  find_item()
}

fn main() {
  do_search()
}
"#
    );
}

#[test]
fn result_in_tail_position_of_result_fn_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn wrapper() -> Result<int, string> {
  get_result()
}

fn main() {
  let _ = wrapper()
}
"#
    );
}

#[test]
fn unused_partial() {
    assert_lint_snapshot!(
        r#"
fn get_partial() -> Partial<int, string> {
  Partial.Ok(42)
}

fn main() {
  get_partial()
  ()
}
"#
    );
}

#[test]
fn unused_partial_in_tail_position() {
    assert_lint_snapshot!(
        r#"
fn get_partial() -> Partial<int, string> {
  Partial.Ok(42)
}

fn main() {
  get_partial()
}
"#
    );
}

#[test]
fn unused_partial_handled_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_partial() -> Partial<int, string> {
  Partial.Ok(42)
}

fn main() {
  let _ = get_partial()
  ()
}
"#
    );
}
