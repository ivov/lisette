use crate::assert_emit_snapshot;

#[test]
fn result_ok_construction() {
    let input = r#"
fn test() -> Result<int, string> {
  Ok(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_err_construction() {
    let input = r#"
fn test() -> Result<int, string> {
  Err("error")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_some_construction() {
    let input = r#"
fn test() -> Option<int> {
  Some(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_none_construction() {
    let input = r#"
fn test() -> Option<int> {
  None
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_int_vs_option_string() {
    let input = r#"
fn test_int() -> Option<int> {
  Some(42)
}

fn test_string() -> Option<string> {
  Some("hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_different_error_types() {
    let input = r#"
fn test_string_error() -> Result<int, string> {
  Ok(42)
}

fn test_int_error() -> Result<string, int> {
  Ok("hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_with_result() {
    let input = r#"
fn test() {
  let x = Ok(42);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_with_option() {
    let input = r#"
fn test() {
  let x = Some(42);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_of_option() {
    let input = r#"
fn test() -> Option<Option<int>> {
  Some(Some(42))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_of_option() {
    let input = r#"
fn test() -> Result<Option<int>, string> {
  Ok(Some(42))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_of_result() {
    let input = r#"
fn test() -> Option<Result<int, string>> {
  Some(Ok(42))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_of_slice() {
    let input = r#"
fn test() -> Option<Slice<int>> {
  Some([1, 2, 3])
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn some_with_named_function_alias_arg() {
    let input = r#"
type Handler = fn(int) -> int

fn double(x: int) -> int {
  x * 2
}

struct Wrapper {
  pub f: Option<Handler>,
}

fn main() {
  let _w = Wrapper { f: Some(double) }
  let _ = _w.f
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_call_with_named_function_alias_arg() {
    let input = r#"
type Handler = fn(int) -> int

fn double(x: int) -> int {
  x * 2
}

struct Box<T> {
  pub v: T,
}

struct Wrap {
  pub b: Box<Handler>,
}

fn make_box<T>(x: T) -> Box<T> {
  Box { v: x }
}

fn main() {
  let _w = Wrap { b: make_box(double) }
  let _ = _w.b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_with_struct() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> Result<Point, string> {
  Ok(Point { x: 10, y: 20 })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn multiple_result_constructions() {
    let input = r#"
fn test(flag: bool) -> Result<int, string> {
  if flag {
    Ok(42)
  } else {
    Err("error")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn multiple_option_constructions() {
    let input = r#"
fn test(flag: bool) -> Option<int> {
  if flag {
    Some(42)
  } else {
    None
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn chained_result_construction() {
    let input = r#"
fn get_value() -> Result<int, string> {
  Ok(42)
}

fn test() -> Result<int, string> {
  let x = get_value();
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn chained_option_construction() {
    let input = r#"
fn get_value() -> Option<int> {
  Some(42)
}

fn test() -> Option<int> {
  let x = get_value();
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_option_in_function() {
    let input = r#"
fn maybe_get() -> Option<int> {
  None
}

fn process() -> Option<int> {
  let x = maybe_get()?;
  Some(x + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_err_with_value_propagation() {
    let input = r#"
fn fallible() -> Result<int, string> {
  Err("something went wrong")
}

fn process() -> Result<int, string> {
  let x = fallible()?;
  Ok(x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_returning_option() {
    let input = r#"
fn test(flag: bool) -> Option<int> {
  match flag {
    true => Some(42),
    false => None,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_returning_result() {
    let input = r#"
fn test(flag: bool) -> Result<int, string> {
  match flag {
    true => Ok(42),
    false => Err("failed"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_assignment_with_some() {
    let input = r#"
fn test() {
  let mut opt: Option<int> = None;
  opt = Some(42);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_assignment_with_none() {
    let input = r#"
fn test() {
  let mut opt: Option<int> = Some(1);
  opt = None;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_assignment_with_ok() {
    let input = r#"
fn test() {
  let mut res: Result<int, string> = Err("initial");
  res = Ok(42);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_assignment_with_err() {
    let input = r#"
fn test() {
  let mut res: Result<int, string> = Ok(1);
  res = Err("error");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_option_construction() {
    let input = r#"
fn test() -> Option<Option<int>> {
  Some(None)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_option_from_variable() {
    let input = r#"
fn test(flag: bool) -> Option<int> {
  let result = if flag { Some(42) } else { None };
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_result_from_variable() {
    let input = r#"
fn test(flag: bool) -> Result<int, string> {
  let result = if flag { Ok(42) } else { Err("nope") };
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assignment_with_regular_value() {
    let input = r#"
fn test() {
  let mut x = 0;
  x = 42;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_from_external_call() {
    let input = r#"
fn external() -> Result<int, string> {
  Ok(1)
}

fn test() -> Result<int, string> {
  external()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_assignment_from_function_call() {
    let input = r#"
fn get_value() -> Option<int> {
  Some(42)
}

fn test() {
  let mut opt: Option<int> = None;
  opt = get_value();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_assignment_from_function_call() {
    let input = r#"
fn get_value() -> Result<int, string> {
  Ok(42)
}

fn test() {
  let mut res: Result<int, string> = Err("initial");
  res = get_value();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_assignment_from_variable() {
    let input = r#"
fn test() {
  let x = Some(42);
  let mut opt: Option<int> = None;
  opt = x;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_on_variable() {
    let input = r#"
fn test() -> Option<int> {
  let x: Option<int> = Some(42);
  let y = x?;
  Some(y + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_on_variable_in_expression() {
    let input = r#"
fn test() -> Option<int> {
  let x: Option<int> = Some(10);
  Some(x? + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_from_option_function() {
    let input = r#"
fn get_value() -> Option<int> {
  Some(42)
}

fn test() {
  let x = get_value();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_returning_option_with_function_call() {
    let input = r#"
fn get_value() -> Option<int> {
  Some(99)
}

fn test(flag: bool) -> Option<int> {
  if flag { Some(42) } else { get_value() }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_with_wildcard_returning_option() {
    let input = r#"
fn test(n: int) -> Option<int> {
  match n {
    1 => Some(10),
    2 => Some(20),
    _ => None,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_on_result_variable() {
    let input = r#"
fn test() -> Result<int, string> {
  let r: Result<int, string> = Ok(42);
  let x = r?;
  Ok(x + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_direct_as_argument() {
    let input = r#"
fn divide(a: int, b: int) -> Result<int, string> {
  if b == 0 { Err("division by zero") } else { Ok(a / b) }
}

fn describe(r: Result<int, string>) -> string {
  match r { Ok(v) => f"{v}", Err(e) => e }
}

fn test() -> string {
  describe(divide(10, 2))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_direct_as_argument() {
    let input = r#"
fn maybe_int(b: bool) -> Option<int> {
  if b { Some(42) } else { None }
}

fn unwrap_or(o: Option<int>, fallback: int) -> int {
  match o { Some(v) => v, None => fallback }
}

fn test() -> int {
  unwrap_or(maybe_int(true), 0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_constructor_as_argument_no_binding() {
    let input = r#"
fn describe(r: Result<int, string>) -> string {
  match r { Ok(v) => f"{v}", Err(e) => e }
}

fn test() -> string {
  describe(Ok(42))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_constructor_as_argument_no_binding() {
    let input = r#"
fn unwrap_or(o: Option<int>, fallback: int) -> int {
  match o { Some(v) => v, None => fallback }
}

fn test() -> int {
  unwrap_or(Some(42), 0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_statement_result_unit_arms() {
    let input = r#"
fn noop() {}

fn divide(a: int, b: int) -> Result<int, string> {
  if b == 0 { Err("err") } else { Ok(a / b) }
}

fn test() {
  match divide(10, 2) {
    Ok(_) => noop(),
    Err(_) => noop(),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_statement_option_unit_arms() {
    let input = r#"
fn noop() {}

fn maybe(b: bool) -> Option<int> {
  if b { Some(42) } else { None }
}

fn test() {
  match maybe(true) {
    Some(_) => noop(),
    None => noop(),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_statement_result_with_binding() {
    let input = r#"
fn use_value(x: int) {}

fn divide(a: int, b: int) -> Result<int, string> {
  if b == 0 { Err("err") } else { Ok(a / b) }
}

fn test() {
  match divide(10, 2) {
    Ok(v) => use_value(v),
    Err(_) => use_value(0),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_option_function_call() {
    let input = r#"
import "go:fmt"

fn next_item(counter: int) -> Option<int> {
  if counter < 5 { Some(counter) } else { None }
}

fn test() {
  let mut i = 0;
  while let Some(x) = next_item(i) {
    fmt.Print(f"Got: {x}\n");
    i = i + 1;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_result_function_call() {
    let input = r#"
import "go:fmt"

fn next_result(counter: int) -> Result<int, string> {
  if counter < 5 { Ok(counter) } else { Err("done") }
}

fn test() {
  let mut i = 0;
  while let Ok(x) = next_result(i) {
    fmt.Print(f"Got: {x}\n");
    i = i + 1;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_in_tuple_literal() {
    let input = r#"
fn maybe_int(x: int) -> Option<int> {
  if x > 0 { Some(x) } else { None }
}

fn maybe_string(s: string) -> Option<string> {
  if s != "" { Some(s) } else { None }
}

fn test() {
  let pair = (maybe_int(5), maybe_string("hello"));
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_in_tuple_literal() {
    let input = r#"
fn try_int(x: int) -> Result<int, string> {
  if x > 0 { Ok(x) } else { Err("negative") }
}

fn test() {
  let pair = (try_int(5), try_int(10));
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_in_array_literal() {
    let input = r#"
fn maybe(x: int) -> Option<int> {
  if x > 0 { Some(x) } else { None }
}

fn test() {
  let arr = [maybe(1), maybe(0), maybe(3), maybe(-1)];
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_in_array_literal() {
    let input = r#"
fn try_value(x: int) -> Result<int, string> {
  if x > 0 { Ok(x) } else { Err("negative") }
}

fn test() {
  let arr = [try_value(1), try_value(-1), try_value(3)];
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_field_init_option_function() {
    let input = r#"
struct Wrapper {
  opt: Option<int>,
}

fn get_opt(b: bool) -> Option<int> {
  if b { Some(42) } else { None }
}

fn test() {
  let w = Wrapper { opt: get_opt(true) };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_field_init_result_function() {
    let input = r#"
struct Container {
  res: Result<int, string>,
}

fn get_res(b: bool) -> Result<int, string> {
  if b { Ok(42) } else { Err("failed") }
}

fn test() {
  let c = Container { res: get_res(true) };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_multiple_field_init_option_functions() {
    let input = r#"
struct MultiWrapper {
  first: Option<int>,
  second: Option<string>,
}

fn get_int(x: int) -> Option<int> {
  if x > 0 { Some(x) } else { None }
}

fn get_string(s: string) -> Option<string> {
  if s != "" { Some(s) } else { None }
}

fn test() {
  let w = MultiWrapper {
    first: get_int(5),
    second: get_string("hello")
  };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_to_unit_result() {
    let input = r#"
fn returns_int() -> Result<int, string> {
  Ok(42)
}

fn returns_unit() -> Result<(), string> {
  let _ = returns_int()?;
  Ok(())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn chained_propagate_option() {
    let input = r#"
fn get_nested(outer: Option<Option<int>>) -> Option<int> {
  let inner = outer?;
  let val = inner?;
  Some(val)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn chained_propagate_result() {
    let input = r#"
fn get_nested(outer: Result<Result<int, string>, string>) -> Result<int, string> {
  let inner = outer?;
  let val = inner?;
  Ok(val)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_err_with_interface_error_type() {
    let input = r#"
struct MyError { msg: string }

impl MyError {
  fn Error(self) -> string { self.msg }
}

fn might_fail() -> Result<int, error> {
  Err(MyError { msg: "oops" })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_call_result_as_tail_expression() {
    let input = r#"
import "go:fmt"

fn print_hello() -> Result<int, error> {
  fmt.Println("hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_with_interface_type_param() {
    let input = r#"
interface Printable {
  fn to_string(self) -> string
}

struct Text { content: string }

impl Text {
  fn to_string(self) -> string { self.content }
}

fn test() {
  let a: Option<Printable> = Some(Text { content: "hello" })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_of_option_interface() {
    let input = r#"
interface Printable {
  fn to_string(self) -> string
}

struct Text { content: string }
struct Number { value: int }

impl Text {
  fn to_string(self) -> string { self.content }
}

impl Number {
  fn to_string(self) -> string { "number" }
}

fn test() {
  let items: Slice<Option<Printable>> = [
    Some(Text { content: "hello" }),
    Some(Number { value: 42 }),
  ]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_function_returning_tuple_and_error_generates_three_variables() {
    let input = r#"
import "go:net"

fn main() {
  match net.SplitHostPort("localhost:8080") {
    Ok((host, port)) => {
      let _ = host
      let _ = port
      ()
    },
    Err(e) => {
      let _ = e
      ()
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn complex_number_with_typed_float_multiplication() {
    let input = r#"
import "go:fmt"

fn main() {
  let imag_part = 4.0
  let c = 3.0 + imag_part * 1.0i
  fmt.Println(f"complex: {c}")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_rebind_uses_old_binding_in_rhs() {
    let input = r#"
import "go:strconv"

fn parse(s: string) -> Result<int, error> {
  strconv.Atoi(s)
}

fn process() -> Result<int, error> {
  let x = 42
  let x = parse(f"{x}")?
  Ok(x + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_bindings_do_not_leak() {
    let input = r#"
import "go:fmt"

fn parse(s: string) -> Result<int, string> {
  if s == "42" { Ok(42) } else { Err("bad") }
}

fn main() {
  let x = 100
  fmt.Println(x)
  let x = 200

  let result = try {
    let x = parse("42")?
    x + 1
  }

  match result {
    Ok(v) => fmt.Println(v),
    Err(e) => fmt.Println(e),
  }

  fmt.Println(x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagation_check_temp_var_no_collision() {
    let input = r#"
import "go:fmt"

fn foo() -> Result<int, string> {
  let x = Ok(1)?
  let check_1 = 7
  fmt.Println(check_1)
  Ok(x)
}

fn main() {
  let _ = foo()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagation_result_temp_var_no_collision() {
    let input = r#"
fn foo() -> Result<int, string> {
  let y = Ok(1)? + 1
  let result_2 = 7
  let _ = result_2
  Ok(y)
}

fn main() {
  let _ = foo()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_result_temp_var_no_collision() {
    let input = r#"
fn foo() -> Result<int, string> {
  let result = try {
    Ok(1)?
  }
  let tryResult_1 = 7
  let _ = tryResult_1
  Ok(0)
}

fn main() {
  let _ = foo()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_unused_binding_result() {
    let input = r#"
fn fallible() -> Result<int, string> { Ok(1) }

fn test() -> Result<(), string> {
  let _x = fallible()?
  Ok(())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_unused_binding_option() {
    let input = r#"
fn maybe() -> Option<int> { Some(1) }

fn test() -> Option<()> {
  let _x = maybe()?
  Some(())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrapped_return_temp_no_collision() {
    let input = r#"
fn foo() -> Option<int> {
  return if true { Some(1) } else { None };
  let tmp_1 = 7;
  let _ = tmp_1;
  None
}

fn main() {
  let _ = foo();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_direct_err_tail_position() {
    let input = r#"
fn f() -> Result<int, string> {
  Err("e")?
}

fn test() -> Result<int, string> {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_direct_none_tail_position() {
    let input = r#"
fn f() -> Option<int> {
  None?
}

fn test() -> Option<int> {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_final_let_unit_result() {
    let input = r#"
fn f() -> Result<(), string> {
  try {
    let x = Ok(1)?
  }
}

fn test() -> Result<(), string> {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_trailing_unit_call() {
    let input = r#"
fn noop() {}

fn f() -> Result<(), string> {
  try {
    let _ = Ok(1)?
    noop()
  }
}

fn test() -> Result<(), string> {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_trailing_while_loop() {
    let input = r#"
fn f() -> Result<(), string> {
  try {
    let _ = Ok(1)?
    while true {
      break
    }
  }
}

fn test() -> Result<(), string> {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_trailing_for_loop() {
    let input = r#"
fn f() -> Result<(), string> {
  try {
    let _ = Ok(1)?
    for i in [1, 2] {
      let _ = i
      break
    }
  }
}

fn test() -> Result<(), string> {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_as_err_constructor_arg() {
    let input = r#"
fn noop() {}

fn test() -> Result<int, ()> {
  Err(noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_trailing_while() {
    let input = r#"
fn test() -> Result<(), PanicValue> {
  recover {
    while true {
      break
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_trailing_while_let() {
    let input = r#"
fn test() -> Result<(), string> {
  try {
    let _ = Ok(1)?
    let o = Some(1)
    while let Some(_v) = o {
      break
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_trailing_assignment() {
    let input = r#"
fn test() -> Result<(), string> {
  let mut x = 0
  let r = try {
    let _ = Ok(1)?
    x = 1
  }
  let _ = x
  r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_ok_return_tail() {
    let input = r#"
fn noop() {}

fn f() -> Result<(), string> {
  Ok(noop())
}

fn main() { let _ = f() }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_ok_constructor_assignment() {
    let input = r#"
fn noop() {}

fn test() -> Result<(), string> {
  let r: Result<(), string> = if true { Ok(noop()) } else { Ok(()) }
  r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_panic_tail_result_context() {
    let input = r#"
fn test() -> Result<int, string> {
  try {
    let _ = Ok(1)?;
    panic("fatal")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_user_never_tail_result_context() {
    let input = r#"
fn die() -> Never { panic("dead") }

fn test() -> Result<int, string> {
  try {
    let _ = Ok(1)?;
    die()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_panic_tail_result_context() {
    let input = r#"
fn test() -> Result<int, PanicValue> {
  recover { panic("fatal") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_panic_tail_option_context() {
    let input = r#"
fn test() -> Option<int> {
  try {
    let _ = Some(1)?;
    panic("fatal")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_try_in_if_arm_with_never_tail() {
    let input = r#"
fn die() -> Never { panic("dead") }

fn test(flag: bool) -> Result<int, string> {
  if flag {
    try {
      let _ = Ok(1)?;
      die()
    }
  } else {
    Ok(42)
  }
}
"#;
    assert_emit_snapshot!(input);
}
