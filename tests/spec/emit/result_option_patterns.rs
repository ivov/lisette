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
fn option_some_types_a_literal_of_another_default() {
    let input = r#"
fn halved(value: Option<float64>) -> float64 {
  match value {
    Some(v) => v / 4.0,
    None => 0.0,
  }
}

fn test() -> float64 {
  let widened: Option<float64> = Some(1)
  halved(widened)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_ok_types_a_literal_of_another_default() {
    let input = r#"
fn test() -> Result<float64, error> {
  let widened: Result<float64, error> = Ok(1)
  widened
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_some_types_a_literal_into_a_newtype() {
    let input = r#"
struct Flag(bool)
struct Ticket(int)

fn test() -> (Option<Flag>, Option<Ticket>) {
  let armed: Option<Flag> = Some(true)
  let ticket: Option<Ticket> = Some(1)
  (armed, ticket)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_some_types_a_constant_builtin_result() {
    let input = r#"
fn test() -> Option<float64> {
  let widened: Option<float64> = Some(min(1, 2))
  widened
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
fn fused_result_match_ok_wildcard() {
    let input = r#"
import "go:errors"

fn fallible(ok: bool) -> Result<int, error> {
  if ok { Ok(1) } else { Err(errors.New("nope")) }
}

fn test() {
  match fallible(true) {
    Ok(_) => {},
    Err(e) => { let _ = e },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_result_match_ok_unused_named_payload() {
    let input = r#"
import "go:errors"

fn fallible(ok: bool) -> Result<int, error> {
  if ok { Ok(1) } else { Err(errors.New("nope")) }
}

fn test() {
  match fallible(true) {
    Ok(x) => {},
    Err(e) => { let _ = e },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_result_match_err_unused_named_payload() {
    let input = r#"
import "go:errors"

fn fallible(ok: bool) -> Result<int, error> {
  if ok { Ok(1) } else { Err(errors.New("nope")) }
}

fn test() {
  match fallible(true) {
    Ok(x) => { let _ = x },
    Err(e) => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_pointer_result_match_unused_err() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test() {
  let file = match os.Create("f") {
    Ok(f) => f,
    Err(e) => {
      fmt.Println("error")
      return
    },
  }
  defer file.Close()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_pointer_result_match_used_err() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test() {
  let file = match os.Create("f") {
    Ok(f) => f,
    Err(e) => {
      fmt.Println(e)
      return
    },
  }
  defer file.Close()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_pointer_result_match_ok_wildcard() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test() {
  match os.Create("f") {
    Ok(_) => fmt.Println("made"),
    Err(e) => fmt.Println(e),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_interface_result_match_uses_nil_interface_guard() {
    let input = r#"
import "go:net"
import "go:fmt"

fn test() {
  match net.Dial("tcp", "addr") {
    Ok(conn) => { let _ = conn },
    Err(e) => fmt.Println(e),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_if_let_err() {
    let input = r#"
import "go:os"

fn test(name: string) -> string {
  if let Err(e) = os.Stat(name) {
    return name
  }
  "found"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_if_let_err_reads_payload() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test(name: string) {
  if let Err(e) = os.Stat(name) {
    fmt.Println(e)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_if_let_ok() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test(name: string) {
  if let Ok(info) = os.Stat(name) {
    fmt.Println(info.Name())
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_if_let_ok_with_else() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test(name: string) {
  if let Ok(info) = os.Stat(name) {
    fmt.Println(info.Name())
  } else {
    fmt.Println("missing")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_if_let_err_on_pointer_return() {
    let input = r#"
import "go:os"

fn test(name: string) -> bool {
  if let Err(e) = os.Open(name) {
    return false
  }
  true
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_let_else_pointer_return() {
    let input = r#"
import "go:net/url"

fn test(raw: string) -> string {
  let Ok(parsed) = url.Parse(raw) else {
    return "bad"
  }
  parsed.Scheme
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_let_else_interface_return() {
    let input = r#"
import "go:os"

fn test(name: string) -> bool {
  let Ok(info) = os.Stat(name) else {
    return false
  }
  info.IsDir()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_let_else_discarded_payload() {
    let input = r#"
import "go:os"

fn test(name: string) -> bool {
  let Ok(_) = os.Stat(name) else {
    return false
  }
  true
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_let_else_without_nil_guard() {
    let input = r#"
import "go:strconv"

fn test(text: string) -> int {
  let Ok(parsed) = strconv.Atoi(text) else {
    return -1
  }
  parsed
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_let_else_shadows_outer_binding() {
    let input = r#"
import "go:strconv"
import "go:fmt"

fn test(text: string) -> int {
  let parsed = "outer"
  fmt.Println(parsed)
  let Ok(parsed) = strconv.Atoi(text) else {
    return -1
  }
  parsed
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_lisette_result_let_else() {
    let input = r#"
import "go:errors"

fn fallible(ok: bool) -> Result<int, error> {
  if ok { Ok(1) } else { Err(errors.New("nope")) }
}

fn test() -> int {
  let Ok(x) = fallible(true) else {
    return -1
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_on_result_value_is_not_fused() {
    let input = r#"
fn test(res: Result<int, string>) -> int {
  let Ok(x) = res else {
    return -1
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_go_result_match_both_arms_empty() {
    let input = r#"
import "go:strconv"

fn test(text: string) {
  match strconv.Atoi(text) {
    Ok(_) => (),
    Err(_) => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_bare_error_match_binds_unit_payload() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test() {
  match os.Remove("f") {
    Ok(x) => { fmt.Println(x) },
    Err(e) => { fmt.Println(e) },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_fused_go_result_match_binds_call_slot() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test(name: string) {
  let file = match os.Open(name) {
    Ok(f) => f,
    Err(e) => {
      fmt.Println("cannot open")
      return
    },
  }
  fmt.Println(file.Name())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_fused_go_result_match_binds_interface_call_slot() {
    let input = r#"
import "go:net"
import "go:fmt"

fn test(addr: string) {
  let conn = match net.Dial("tcp", addr) {
    Ok(c) => c,
    Err(e) => {
      fmt.Println("cannot dial")
      return
    },
  }
  let _ = conn
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_match_with_non_diverging_err_arm_keeps_declaration() {
    let input = r#"
import "go:strconv"

fn test(text: string) -> int {
  let parsed = match strconv.Atoi(text) {
    Ok(n) => n,
    Err(e) => -1,
  }
  parsed
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_match_with_computed_ok_arm_keeps_declaration() {
    let input = r#"
import "go:os"
import "go:fmt"

fn test(name: string) {
  let label = match os.Open(name) {
    Ok(f) => f.Name(),
    Err(e) => {
      fmt.Println("cannot open")
      return
    },
  }
  fmt.Println(label)
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
  fn to_string() -> string
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
fn option_with_unknown_type_param() {
    let input = r#"
fn take(value: Option<Unknown>) -> bool {
  value.is_some()
}

fn test() {
  let boxed: Option<Unknown> = Some(1)
  if !take(boxed) {
    panic("Option<Unknown> lost its widened type argument")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_of_option_interface() {
    let input = r#"
interface Printable {
  fn to_string() -> string
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
fn lisette_function_returning_result_tuple_uses_packed_abi() {
    let input = r#"
fn pair<A, B>(a: A, b: B) -> Result<(A, B), error> {
  Ok((a, b))
}

fn test() {
  match pair<int, string>(1, "x") {
    Ok((first, second)) => {
      let _ = first
      let _ = second
    },
    Err(e) => {
      let _ = e
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
  let tmp_1 = 7;
  let _ = tmp_1;
  return if true { Some(1) } else { None };
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
fn tail_panic_in_result_returning_function() {
    let input = r#"
fn forbidden() -> Result<int, error> {
  panic("boom")
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

#[test]
fn propagate_direct_err_lowered_result_tuple() {
    let input = r#"
import "go:errors"

fn fail() -> Result<int, error> {
  Err(errors.New("boom"))?
  Ok(1)
}

fn main() {
  let _ = fail()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_direct_none_lowered_option_comma_ok() {
    let input = r#"
fn missing() -> Option<int> {
  None?
  Some(1)
}

fn main() {
  let _ = missing()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_direct_err_lowered_bare_error() {
    let input = r#"
import "go:errors"

fn fail_unit() -> Result<(), error> {
  Err(errors.New("boom"))?
  Ok(())
}

fn main() {
  let _ = fail_unit()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_propagation() {
    let input = r#"
fn load(r: Result<int, error>) -> Result<int, error> {
  let n = r.wrap_err("loading config")?
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_runtime_wraps_message() {
    let input = r#"
import "go:errors"

fn test() {
  let r: Result<int, error> = Err(errors.New("boom"))
  match r.wrap_err("loading config") {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "loading config: boom" {
        panic("wrong wrapped message")
      }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_widens_concrete_error_in_lowered_return() {
    let input = r#"
struct ValidationError { field: string }

impl ValidationError {
  fn Error(self) -> string { f"{self.field}: required" }
}

fn validate(name: string) -> Result<string, ValidationError> {
  if name == "" { return Err(ValidationError { field: "name" }) }
  Ok(name)
}

fn load(name: string) -> Result<string, error> {
  let n = validate(name)?
  Ok(n)
}

fn test() {
  match load("") {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "name: required" {
        panic("wrong widened error")
      }
    },
  }
  match load("ada") {
    Ok(v) => {
      if v != "ada" {
        panic("wrong ok value")
      }
    },
    Err(_) => panic("expected ok"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_widens_in_annotated_try_block() {
    let input = r#"
struct AError { }

impl AError {
  fn Error(self) -> string { "a failed" }
}

struct BError { }

impl BError {
  fn Error(self) -> string { "b failed" }
}

fn do_a(ok: bool) -> Result<int, AError> {
  if ok { Ok(1) } else { Err(AError {}) }
}

fn do_b(ok: bool) -> Result<int, BError> {
  if ok { Ok(2) } else { Err(BError {}) }
}

fn test() {
  let r: Result<int, error> = try {
    let a = do_a(true)?
    let b = do_b(false)?
    a + b
  }
  match r {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "b failed" {
        panic("wrong try block error")
      }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_widens_in_prelude_callback_lambda() {
    let input = r#"
struct ParseError { text: string }

impl ParseError {
  fn Error(self) -> string { f"bad: {self.text}" }
}

fn parse_word(w: string) -> Result<int, ParseError> {
  if w == "x" { Err(ParseError { text: w }) } else { Ok(w.length()) }
}

fn test() {
  let words = ["one", "x"]
  let parsed = words.map(|w| -> Result<int, error> {
    let n = parse_word(w)?
    Ok(n)
  })
  match parsed[0] {
    Ok(n) => {
      if n != 3 {
        panic("wrong parsed length")
      }
    },
    Err(_) => panic("expected ok"),
  }
  match parsed[1] {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "bad: x" {
        panic("wrong lambda error")
      }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_widens_to_custom_interface_with_different_ok_types() {
    let input = r#"
pub interface AppError {
  fn Error() -> string
  fn status() -> int
}

struct DbError { }

impl DbError {
  fn Error(self) -> string { "db down" }
  pub fn status(self) -> int { 500 }
}

fn query(ok: bool) -> Result<string, DbError> {
  if ok { Ok("row") } else { Err(DbError {}) }
}

fn handler(ok: bool) -> Result<int, AppError> {
  let row = query(ok)?
  Ok(row.length())
}

fn test() {
  match handler(false) {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.status() != 500 || e.Error() != "db down" {
        panic("wrong app error")
      }
    },
  }
  match handler(true) {
    Ok(n) => {
      if n != 3 {
        panic("wrong ok length")
      }
    },
    Err(_) => panic("expected ok"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn err_literal_propagate_widens() {
    let input = r#"
struct AError { }

impl AError {
  fn Error(self) -> string { "a failed" }
}

fn bail(flag: bool) -> Result<int, error> {
  if flag { Err(AError {})? }
  Ok(1)
}

fn test() {
  match bail(true) {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "a failed" {
        panic("wrong literal error")
      }
    },
  }
  match bail(false) {
    Ok(v) => {
      if v != 1 {
        panic("wrong ok value")
      }
    },
    Err(_) => panic("expected ok"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn return_err_widens_concrete_error() {
    let input = r#"
struct AError { }

impl AError {
  fn Error(self) -> string { "a failed" }
}

fn bail() -> Result<int, error> {
  return Err(AError {})
}

fn test() {
  match bail() {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "a failed" {
        panic("wrong returned error")
      }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_widens_ref_with_pointer_receiver_error_method() {
    let input = r#"
struct FileError { path: string }

impl FileError {
  fn Error(self: Ref<FileError>) -> string { f"cannot open {self.path}" }
}

fn read_value() -> Result<int, Ref<FileError>> { Err(&FileError { path: "a.txt" }) }

fn load() -> Result<int, error> {
  let n = read_value()?
  Ok(n)
}

fn test() {
  match load() {
    Ok(_) => panic("expected error"),
    Err(e) => {
      if e.Error() != "cannot open a.txt" {
        panic("wrong ref error")
      }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_pairs_take_distinct_status_names_in_one_block() {
    let input = r#"
import "go:os"
import "go:strconv"

fn run(path: string) -> Result<int, error> {
  let text = os.ReadFile(path)?
  let Ok(n) = strconv.Atoi(os.Getenv("N")) else { return Ok(text.length()) }
  os.Remove(path)?
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_status_avoids_a_live_err_binding() {
    let input = r#"
import "go:errors"
import "go:strconv"

fn run() -> Result<int, error> {
  let err = errors.New("outer")
  let n = strconv.Atoi("1")?
  if n == 0 { return Err(err) }
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_status_takes_the_err_arm_name() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  match strconv.Atoi("1") {
    Ok(n) => fmt.Println(n),
    Err(failure) => fmt.Println(failure),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_status_only_call_opens_the_if_initializer() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  if let Err(err) = os.Remove("a") {
    fmt.Println(err)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_status_shadows_err_inside_a_nested_block() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let first = match strconv.Atoi("1") {
    Ok(n) => n,
    Err(_) => { return },
  }
  if first > 0 {
    let second = match strconv.Atoi("2") {
      Ok(n) => n,
      Err(err) => {
        fmt.Println(err)
        return
      },
    }
    fmt.Println(second)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_status_of_another_kind_is_not_redeclared() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn run(m: Map<string, int>) {
  let parsed = match strconv.Atoi("1") {
    Ok(n) => n,
    Err(ok) => {
      fmt.Println(ok)
      return
    },
  }
  let Some(v) = m.get("a") else { return }
  fmt.Println(parsed, v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn value_position_match_assigns_from_the_if_initializer() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let doubled = match strconv.Atoi("21") {
    Ok(n) => n * 2,
    Err(_) => 0,
  }
  fmt.Println(doubled)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_match_on_nullable_call_names_the_value() {
    let input = r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  match ctx.Err() {
    Some(err) => fmt.Println(err),
    None => fmt.Println("no error"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ok_binding_shadowing_a_live_name_gets_a_fresh_header_name() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let n = 1
  match strconv.Atoi("2") {
    Ok(n) => fmt.Println(n),
    Err(_) => fmt.Println(n),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ok_and_err_arms_sharing_a_name_get_distinct_slots() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  match strconv.Atoi("2") {
    Ok(x) => fmt.Println(x),
    Err(x) => fmt.Println(x),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_match_aliases_the_both_arm_names() {
    let input = r#"
import "go:fmt"
import "go:os"

fn write_all(file: Ref<os.File>) {
  match file.Write(['h', 'i']) {
    Ok(n) => fmt.Println(n),
    Both(m, e) => fmt.Println(m, e),
    Err(e) => fmt.Println(e),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_named_like_its_ok_binding_fuses() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let bytes = match os.ReadFile("tasks.json") {
    Ok(bytes) => bytes,
    Err(err) => {
      fmt.Println(err)
      return
    },
  }
  fmt.Println(bytes.length())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_named_like_its_ok_binding_keeps_an_inner_let_apart() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let bytes = match os.ReadFile("tasks.json") {
    Ok(bytes) => bytes,
    Err(err) => {
      let bytes = err.Error().bytes()
      fmt.Println(bytes.length())
      return
    },
  }
  fmt.Println(bytes.length())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_named_like_its_some_binding_fuses() {
    let input = r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  let err = match ctx.Err() {
    Some(err) => err,
    None => { return },
  }
  fmt.Println(err)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_err_on_go_call_opens_the_if_header() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  if os.Remove("missing.txt").is_err() {
    fmt.Println("nothing to remove")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negated_is_ok_on_go_call_flips_the_nil_test() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  if !strconv.Atoi("x").is_ok() {
    fmt.Println("not a number")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_ok_on_pointer_returning_go_call_keeps_the_nil_guard() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  if os.Open("a").is_ok() {
    fmt.Println("readable")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_err_on_lisette_result_calls_opens_the_if_header() {
    let input = r#"
import "go:fmt"

fn count() -> Result<int, error> { Ok(1) }
fn touch() -> Result<(), error> { Ok(()) }

fn main() {
  if count().is_err() {
    fmt.Println("count failed")
  }
  if touch().is_err() {
    fmt.Println("touch failed")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_some_on_map_get_opens_the_if_header() {
    let input = r#"
import "go:fmt"

fn main() {
  let m = Map.new<string, int>()
  if m.get("a").is_some() {
    fmt.Println("has a")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_none_on_nullable_go_call_opens_the_if_header() {
    let input = r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  if ctx.Err().is_none() {
    fmt.Println("clean")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_ok_as_a_value_binds_the_call_first() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let raw = "8080"
  let succeeded = strconv.Atoi(raw).is_ok()
  let failed = !strconv.Atoi(raw).is_ok()
  fmt.Println(succeeded, failed)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn predicate_value_stays_inline_when_a_later_sibling_binds_err() {
    let input = r#"
fn first() -> Result<int, error> { Ok(1) }
fn second() -> Result<int, error> { Ok(2) }
fn pick(flag: bool, n: int) -> int { if flag { n } else { 0 } }

fn run() -> Result<int, error> {
  Ok(pick(first().is_ok(), second()?))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_ok_in_a_while_condition_runs_the_call_each_iteration() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let mut text = "1"
  while strconv.Atoi(text).is_ok() {
    text = text + "x"
  }
  fmt.Println(text)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_ok_under_logical_and_stays_an_expression() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let ready = true
  if ready && strconv.Atoi("1").is_ok() {
    fmt.Println("both")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_ok_on_partial_keeps_the_tagged_path() {
    let input = r#"
import "go:fmt"
import "go:os"

fn write_all(file: Ref<os.File>) {
  if file.Write(['h', 'i']).is_ok() {
    fmt.Println("written")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_literal_receiver_in_an_if_header() {
    let input = r#"
import "go:fmt"

struct Parser { src: string }

impl Parser {
  fn parse(self: Parser) -> Result<int, error> { Ok(self.src.length()) }
}

fn main() {
  match Parser { src: "ab" }.parse() {
    Ok(n) => fmt.Println(n),
    Err(err) => fmt.Println(err),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_propagate_on_go_calls_uses_errorf() {
    let input = r#"
import "go:encoding/json"
import "go:fmt"
import "go:os"

const FILE = "nums.json"

fn load() -> Result<Slice<int>, error> {
  let bytes = os.ReadFile(FILE).wrap_err(f"reading {FILE}")?
  let mut nums: Slice<int> = []
  json.Unmarshal(bytes, &nums).wrap_err("parsing nums")?
  Ok(nums)
}

fn main() {
  fmt.Println(load())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_message_escapes_percent_and_interpolates_values() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn parse(raw: string, attempt: int) -> Result<int, error> {
  let n = strconv.Atoi(raw).wrap_err(f"attempt {attempt} at 100% of {raw}")?
  Ok(n)
}

fn main() {
  fmt.Println(parse("1", 2))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_computed_message_uses_a_string_verb() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn describe(raw: string) -> string { "parsing " + raw }

fn parse(raw: string) -> Result<int, error> {
  let n = strconv.Atoi(raw).wrap_err(describe(raw))?
  Ok(n)
}

fn main() {
  fmt.Println(parse("1"))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_on_pointer_returning_go_call_wraps_both_failures() {
    let input = r#"
import "go:fmt"
import "go:os"

fn open(path: string) -> Result<Ref<os.File>, error> {
  let file = os.Open(path).wrap_err("open")?
  Ok(file)
}

fn main() {
  fmt.Println(open("missing.txt"))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn err_constructor_wrap_err_in_return_position() {
    let input = r#"
import "go:errors"
import "go:fmt"
import "go:io/fs"
import "go:os"

const FILE = "tasks.json"

fn load() -> Result<Slice<byte>, error> {
  let bytes = match os.ReadFile(FILE) {
    Ok(bytes) => bytes,
    Err(err) => {
      if errors.Is(err, fs.ErrNotExist) { return Ok([]) }
      return Err(err).wrap_err(f"reading {FILE}")
    },
  }
  Ok(bytes)
}

fn main() {
  fmt.Println(load())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_wrap_err_of_go_call_wraps_in_the_err_arm() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  match strconv.Atoi("x").wrap_err("parse") {
    Ok(n) => fmt.Println(n),
    Err(err) => fmt.Println(err),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tail_return_of_wrap_err_on_go_call() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn port(raw: string) -> Result<int, error> {
  strconv.Atoi(raw).wrap_err("read port")
}

fn main() {
  fmt.Println(port("8080"))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_wrap_err_wraps_inner_message_first() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn parse(raw: string) -> Result<int, error> {
  let n = strconv.Atoi(raw).wrap_err("inner").wrap_err("outer")?
  Ok(n)
}

fn main() {
  fmt.Println(parse("1"))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unwrap_or_on_comma_ok_go_call_assigns_the_default_on_failure() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let home = os.LookupEnv("HOME").unwrap_or("unset")
  fmt.Println(home)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unwrap_or_on_go_result_call_assigns_the_default_on_error() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let port = strconv.Atoi("x").unwrap_or(80)
  fmt.Println(port)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unwrap_or_on_map_get_as_an_argument() {
    let input = r#"
import "go:fmt"

fn main() {
  let counts = Map.new<string, int>()
  fmt.Println(counts.get("a").unwrap_or(0) + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_then_unwrap_or_on_comma_ok_go_call_branches_on_the_header() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let shown = os.LookupEnv("HOME").map(|h| "home=" + h).unwrap_or("unset")
  fmt.Println(shown)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_or_on_nullable_go_call_as_an_argument() {
    let input = r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  fmt.Println(ctx.Err().map_or("clean", |err| err.Error()))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_with_a_returning_lambda_keeps_the_tagged_path() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let shown = os.LookupEnv("HOME").map(|h| { return "home=" + h }).unwrap_or("unset")
  fmt.Println(shown)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_alone_keeps_the_option() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let shown = os.LookupEnv("HOME").map(|h| "home=" + h)
  fmt.Println(shown.is_some())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unwrap_or_on_a_stored_option_keeps_the_tagged_path() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let home = os.LookupEnv("HOME")
  fmt.Println(home.unwrap_or("unset"))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn some_call_into_nullable_go_parameter_passes_the_value() {
    let input = r#"
import "go:fmt"
import "go:net/http"
import "go:strings"

fn main() {
  let req = http.NewRequest("POST", "https://example.com", Some(strings.NewReader("hello")))
  fmt.Println(req.is_ok())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_variable_into_nullable_go_parameter_keeps_the_tag_test() {
    let input = r#"
import "go:fmt"
import "go:net/http"
import "go:os"

fn main() {
  let body = if os.Args.length() > 1 { Some(os.Stdin) } else { None }
  let req = http.NewRequest("POST", "https://example.com", body)
  fmt.Println(req.is_ok())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn some_into_nullable_go_field_assignment_passes_the_value() {
    let input = r#"
import "go:fmt"
import "go:net/http"

fn main() {
  let mut server = http.Server { Addr: ":8080", .. }
  server.Handler = Some(http.NewServeMux())
  fmt.Println(server.Addr)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_a_field_path_reads_the_field_in_place() {
    let input = r#"
import "go:fmt"

enum Status { Pending, Done }

struct Task { status: Status }

impl Task {
  fn icon(self: Ref<Task>) -> string {
    match self.status {
      Status.Pending => "[ ]",
      Status.Done => "[x]",
    }
  }
}

fn main() {
  let t = Task { status: Status.Pending }
  fmt.Println(t.icon())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_a_field_path_with_a_guard_keeps_the_subject_temp() {
    let input = r#"
import "go:fmt"

struct Task { retries: Option<int> }

fn describe(t: Ref<Task>) -> string {
  match t.retries {
    Some(n) if n > 3 => "many",
    Some(_) => "some",
    None => "none",
  }
}

fn main() {
  fmt.Println(describe(&Task { retries: Some(1) }))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_a_field_path_whose_arm_rebinds_the_root_keeps_the_subject_temp() {
    let input = r#"
import "go:fmt"

struct Node { next: Option<int> }

fn main() {
  let t = Node { next: Some(2) }
  match t.next {
    Some(t) => fmt.Println(t),
    None => fmt.Println("end"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_on_a_field_path_reads_the_field_in_place() {
    let input = r#"
import "go:fmt"

struct Task { parent: Option<int> }

fn parent_of(item: Ref<Task>) -> int {
  let Some(id) = item.parent else { return -1 }
  id
}

fn main() {
  fmt.Println(parent_of(&Task { parent: Some(7) }))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_on_a_field_path_reads_the_field_each_iteration() {
    let input = r#"
import "go:fmt"

struct Cursor { next: Option<int> }

fn main() {
  let mut cursor = Cursor { next: Some(3) }
  while let Some(n) = cursor.next {
    fmt.Println(n)
    cursor.next = if n > 0 { Some(n - 1) } else { None }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn find_result_in_a_let_else_needs_no_subject_temp() {
    let input = r#"
import "go:fmt"

fn main() {
  let nums = [1, 2, 3]
  let Some(even) = nums.find(|n| n % 2 == 0) else { return }
  fmt.Println(even)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_bound_over_an_unmutated_slice_reads_len_in_the_header() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let args = os.Args
  for i in 1..args.length() {
    fmt.Println(args[i])
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_bound_over_a_mutated_slice_keeps_the_bound_temp() {
    let input = r#"
import "go:fmt"

fn main() {
  let mut items = [1, 2, 3]
  for i in 0..items.length() {
    items = items.append(i)
    fmt.Println(items.length())
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_map_fills_the_let_name_as_the_result() {
    let input = r#"
import "go:fmt"

struct Task { id: int, done: bool }

fn main() {
  let tasks = [Task { id: 1, done: false }, Task { id: 2, done: true }]
  let ids = tasks.map(|t| t.id)
  let open = tasks.filter(|t| !t.done)
  let total = tasks.fold(0, |sum, t| sum + t.id)
  fmt.Println(ids, open.length(), total)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_map_with_a_returning_lambda_keeps_the_helper_call() {
    let input = r#"
import "go:fmt"

fn main() {
  let nums = [1, 2, 3]
  let doubled = nums.map(|n| { return n * 2 })
  fmt.Println(doubled)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_propagate_in_call_args_binds_the_inner_pair_first() {
    let input = r#"
import "go:strconv"

fn double(n: int) -> Result<int, error> { Ok(n * 2) }

fn run() -> Result<int, error> {
  let d = double(strconv.Atoi("1")?)?
  Ok(d)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_on_slice_get_tests_bounds_and_indexes() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let Some(command) = os.Args.get(1) else {
    fmt.Println("usage: app <command>")
    return
  }
  fmt.Println(command)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_slice_get_with_a_variable_index_guards_both_bounds() {
    let input = r#"
fn pick(xs: Slice<int>, i: int) -> int {
  match xs.get(i) {
    Some(v) => v,
    None => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_on_array_get_indexes_the_array_directly() {
    let input = r#"
fn first(a: Array<int, 3>, i: int) -> int {
  if let Some(v) = a.get(i) { v } else { 0 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_get_with_call_operands_pins_them_once() {
    let input = r#"
fn xs() -> Slice<int> { [1, 2] }
fn idx() -> int { 1 }

fn run() -> int {
  let Some(v) = xs().get(idx()) else { return 0 }
  v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn is_some_on_slice_get_is_a_bounds_test() {
    let input = r#"
fn has(xs: Slice<int>, i: int) -> bool { xs.get(i).is_some() }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_get_through_a_ref_receiver_derefs_once() {
    let input = r#"
fn head(xs: Ref<Slice<int>>) -> int {
  let Some(v) = xs.get(0) else { return 0 }
  v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_on_slice_get_reads_after_the_bounds_test() {
    let input = r#"
fn sum(xs: Slice<int>) -> int {
  let mut i = 0
  let mut total = 0
  while let Some(x) = xs.get(i) {
    total = total + x
    i = i + 1
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_match_on_natives_binds_the_let_name_directly() {
    let input = r#"
fn pick(xs: Slice<int>, i: int) -> int {
  let v = match xs.get(i) { Some(v) => v, None => return -1 }
  v + 1
}

fn lookup(m: Map<string, int>, k: string) -> int {
  let v = match m.get(k) { Some(v) => v, None => return -1 }
  v + 1
}

fn first_even(nums: Slice<int>) -> int {
  let n = match nums.find(|n| n % 2 == 0) { Some(n) => n, None => return -1 }
  n + 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_find_uses_a_found_flag() {
    let input = r#"
fn first_even(nums: Slice<int>) -> int {
  match nums.find(|n| n % 2 == 0) {
    Some(n) => n,
    None => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unwrap_or_evaluates_an_effectful_default_before_the_error_test() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn fallback() -> int {
  fmt.Println("fallback ran")
  7
}

fn one() -> int {
  strconv.Atoi("1").unwrap_or(fallback())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_or_with_an_effectful_default_keeps_the_helper_call() {
    let input = r#"
import "go:fmt"
import "go:os"

fn fallback() -> int {
  fmt.Println("fallback ran")
  0
}

fn home_length() -> int {
  os.LookupEnv("HOME").map_or(fallback(), |home| home.length())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_evaluates_an_effectful_message_before_the_error_test() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn message() -> string {
  fmt.Println("message ran")
  "ctx"
}

fn two() -> Result<int, error> {
  let n = strconv.Atoi("2").wrap_err(message())?
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_wrap_and_default_arguments_preserve_eager_order() {
    let input = r#"
import "go:strconv"

fn choose(raw: string) -> int {
  let mut trace = ""
  let message = || -> string {
    trace = trace + "message;"
    "context"
  }
  let fallback = || -> int {
    trace = trace + "default;"
    7
  }
  let n = strconv.Atoi(raw).wrap_err(message()).unwrap_or(fallback())
  if trace != "message;default;" { panic(trace) }
  n
}

fn main() {
  if choose("1") != 1 || choose("bad") != 7 { panic("wrong result") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_wrap_evaluates_custom_display_on_success_and_failure() {
    let input = r#"
import "go:strconv"

#[display]
struct Label { describe: fn() -> string }

impl Label {
  fn string(self) -> string {
    self.describe()
  }
}

fn parse(raw: string, label: Label) -> Result<int, error> {
  let n = strconv.Atoi(raw).wrap_err(f"took {label}")?
  Ok(n)
}

fn main() {
  let mut seen = 0
  let describe = || -> string {
    seen = seen + 1
    "label"
  }
  let label = Label { describe }
  let good = parse("1", label).unwrap_or(0)
  if good != 1 || seen != 1 { panic("success skipped display") }
  let bad = parse("bad", label).is_ok()
  if bad || seen != 2 { panic("failure skipped display") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn eager_defaults_preserve_numeric_types() {
    let input = r#"
import "go:strconv"

fn shifted(sh: uint64) -> uint64 {
  strconv.ParseUint("bad", 10, 64).unwrap_or(1 << sh)
}

fn negative() -> int64 {
  strconv.ParseInt("bad", 10, 64).unwrap_or(-1)
}

fn main() {
  if shifted(63) != 9223372036854775808 || negative() != -1 { panic("wrong default") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_message_with_a_nested_propagate_keeps_the_outer_error() {
    let input = r#"
import "go:strconv"

fn three() -> Result<int, error> {
  let n = strconv.Atoi("x").wrap_err(f"{strconv.Atoi("0")?}")?
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn get_with_a_constant_index_go_rejects_keeps_the_helper() {
    let input = r#"
fn past_the_end(xs: Array<int, 2>) -> int {
  match xs.get(2) {
    Some(v) => v,
    None => -1,
  }
}

fn negative(xs: Slice<int>) -> int {
  match xs.get(-1) {
    Some(v) => v,
    None => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn adjacent_find_matches_take_distinct_payload_names() {
    let input = r#"
import "go:fmt"

fn five(xs: Slice<int>) -> int {
  if let Some(x) = xs.find(|n| n > 1) {
    fmt.Println(x)
  }
  if let Some(x) = xs.find(|n| n > 2) {
    fmt.Println(x)
  }
  0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unwrap_or_default_with_a_nested_propagate_keeps_the_outer_error() {
    let input = r#"
import "go:strconv"

fn one() -> Result<int, error> {
  Ok(strconv.Atoi("bad").unwrap_or(strconv.Atoi("7")?))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrap_err_messages_run_where_nothing_reads_the_wrapped_error() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn message() -> string {
  fmt.Println("message ran")
  "ctx"
}

fn predicate() -> bool {
  strconv.Atoi("1").wrap_err(message()).is_err()
}

fn defaulted() -> int {
  strconv.Atoi("1").wrap_err(message()).unwrap_or(0)
}

fn let_else() -> int {
  let Ok(v) = strconv.Atoi("1").wrap_err(message()) else { return -1 }
  v
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn shift_defaults_and_display_interpolations_run_before_the_error_test() {
    let input = r#"
import "go:strconv"
import "go:time"

fn three(d: time.Duration, sh: int) -> Result<int, error> {
  let a = strconv.Atoi("1").wrap_err(f"took {d}")?
  let b = strconv.Atoi("2").unwrap_or(1 << sh)
  Ok(a + b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn chained_wrap_err_reads_an_earlier_message_before_a_later_effect() {
    let input = r#"
import "go:strconv"

fn four(initial: string) -> Result<int, error> {
  let mut text = initial
  let update = || -> string {
    text = "changed"
    "outer"
  }
  let n = strconv.Atoi("bad").wrap_err(text).wrap_err(update())?
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrapped_error_constructor_evaluates_its_argument_first() {
    let input = r#"
import "go:errors"
import "go:fmt"

fn message() -> string {
  fmt.Println("message ran")
  "ctx"
}

fn make_error() -> error {
  fmt.Println("make_error ran")
  errors.New("boom")
}

fn five() -> Result<int, error> {
  Err(make_error()).wrap_err(message())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn adjacent_wrapped_matches_take_fresh_payload_names() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn message() -> string {
  fmt.Println("message ran")
  "ctx"
}

fn six() -> int {
  let a = match strconv.Atoi("1").wrap_err(message()) {
    Ok(n) => n,
    Err(e) => {
      fmt.Println(e)
      0
    },
  }
  let b = match strconv.Atoi("2").wrap_err(message()) {
    Ok(n) => n,
    Err(e) => {
      fmt.Println(e)
      0
    },
  }
  a + b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defaulted_let_that_calls_a_function_of_its_own_name_keeps_a_temp() {
    let input = r#"
import "go:strconv"

fn n() -> int { 9 }

fn seven() -> int {
  let n = strconv.Atoi("bad").unwrap_or(n())
  n
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_header_parenthesizes_a_nested_generic_receiver() {
    let input = r#"
struct Box<T> { v: T }

impl<T> Box<T> {
  fn parse(self) -> Option<int> { Some(1) }
}

fn eight() -> int {
  if let Some(v) = Box { v: [1] }.parse() { v } else { 0 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn wrapped_error_variable_is_read_before_a_message_that_rebinds_it() {
    let input = r#"
import "go:errors"

fn two() -> Result<int, error> {
  let mut e = errors.New("before")
  let update = || -> string {
    e = errors.New("after")
    "context"
  }
  Err(e).wrap_err(update())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_map_over_a_function_of_its_own_name_keeps_a_temp() {
    let input = r#"
fn items() -> Slice<int> { [1, 2, 3] }

fn three() -> int {
  let items = items().filter(|x| x > 1)
  items.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn propagate_message_that_calls_a_function_of_the_let_name_keeps_a_temp() {
    let input = r#"
import "go:strconv"

fn n() -> int { 9 }

fn four() -> Result<int, error> {
  let n = strconv.Atoi("bad").wrap_err(f"{n()}")?
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn err_arm_closure_keeps_its_error_past_a_later_pair() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn message() -> string { "context" }

fn parse() -> Result<int, error> {
  let show = match strconv.Atoi("bad").wrap_err(message()) {
    Ok(_) => || fmt.Println("ok"),
    Err(err) => || fmt.Println(err),
  }
  let n = strconv.Atoi("2")?
  let _ = show()
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pairs_of_different_error_types_take_distinct_statuses() {
    let input = r#"
import "go:strconv"

pub interface AppError {
  fn Error() -> string
  fn status() -> int
}

fn first() -> Result<int, AppError> { Ok(1) }

fn run() -> Result<int, error> {
  let a = first()?
  let b = strconv.Atoi("2")?
  Ok(a + b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ignored_error_arm_still_runs_an_effectful_wrap_message() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn message() -> string {
  fmt.Println("message")
  "context"
}

fn main() {
  match strconv.Atoi("2").wrap_err(message()) {
    Ok(n) => fmt.Println(n),
    Err(_) => fmt.Println("bad"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn result_binding_sites_keep_a_temp_when_a_message_reads_the_let_name() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn n() -> string { "context" }

fn let_else() {
  let Ok(n) = strconv.Atoi("2").wrap_err(n()) else { return }
  fmt.Println(n)
}

fn let_match() {
  let n = match strconv.Atoi("2").wrap_err(n()) {
    Ok(x) => x,
    Err(_) => { return },
  }
  fmt.Println(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generated_names_avoid_package_functions() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn err() -> int { 3 }
fn n() -> string { "context" }
fn items() -> Slice<int> { [1, 2] }

fn status() -> Result<int, error> {
  let count = strconv.Atoi("2")?
  Ok(count + err())
}

fn arm_after_message() {
  match strconv.Atoi("2").wrap_err(n()) {
    Ok(n) => fmt.Println(n),
    Err(e) => fmt.Println(e),
  }
}

fn arm_in_other_arm() {
  match strconv.Atoi("bad") {
    Ok(n) => fmt.Println(n),
    Err(_) => fmt.Println(n()),
  }
}

fn find_arm() {
  match items().find(|x| x > 1) {
    Some(items) => fmt.Println(items),
    None => fmt.Println("none"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generated_names_avoid_renamed_package_functions() {
    let input = r#"
import "go:fmt"
import "go:strconv"

fn get_items() -> Slice<int> { [1, 2, 3] }

fn len() -> string { "context" }

fn direct_let() -> int {
  let getItems = 5
  getItems + get_items().length()
}

fn filtered_let() -> int {
  let getItems = get_items().filter(|x| x > 1)
  getItems.length() + get_items().length()
}

fn parameter(getItems: int) -> int {
  let (first, rest) = (getItems, 2)
  first + rest + get_items().length()
}

fn arm_in_other_arm() {
  match strconv.Atoi("bad") {
    Ok(len) => fmt.Println(len),
    Err(_) => fmt.Println(len()),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_bound_over_a_channel_length_keeps_the_bound_temp() {
    let input = r#"
import "go:fmt"

fn six() {
  let ch = Channel.buffered<int>(3)
  ch.send(1)
  ch.send(2)
  ch.send(3)
  let mut iterations = 0
  for _i in 1..ch.length() {
    let _ = ch.receive()
    iterations += 1
  }
  fmt.Println("iterations", iterations)
}
"#;
    assert_emit_snapshot!(input);
}
