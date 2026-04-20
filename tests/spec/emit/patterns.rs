use crate::assert_emit_snapshot;
use crate::assert_emit_snapshot_with_go_typedefs;

#[test]
fn tuple_struct_pattern_in_match() {
    let input = r#"
struct Pair(int, int)

fn test(p: Pair) -> int {
  match p {
    Pair(a, b) => a + b,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_tuple_struct_pattern() {
    let input = r#"
struct Box<T>(T)

fn test(b: Box<int>) -> int {
  match b {
    Box(x) => x,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct_function_field_or_pattern() {
    let input = r#"
struct Handler<T> { callback: fn(T) -> int }
enum E { A(Handler<string>), B(Handler<string>) }

fn test(e: E) -> int {
  let A(Handler { callback }) | B(Handler { callback }) = e else { return 0; };
  callback("test")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct_tuple_field_or_pattern() {
    let input = r#"
struct Pair<T> { coords: (T, T) }
enum E { A(Pair<int>), B(Pair<int>) }

fn test(e: E) -> int {
  let A(Pair { coords: (x, y) }) | B(Pair { coords: (x, y) }) = e else { return 0; };
  x + y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_pattern_rest_or_pattern() {
    let input = r#"
enum E { A(Slice<int>), B(Slice<int>) }

fn test(e: E) -> int {
  let A([first, ..rest]) | B([first, ..rest]) = e else { return 0; };
  first
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_pattern_unused_rest() {
    let input = r#"
fn test(nums: Slice<int>) -> int {
  match nums {
    [first, ..rest] => first,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_variant_pattern_match() {
    let input = r#"
enum Message {
  Move { x: int, y: int },
  Quit,
}

fn handle(m: Message) -> int {
  match m {
    Move { x, y } => x + y,
    Quit => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_variant_pattern_partial() {
    let input = r#"
enum Shape {
  Rectangle { x: int, y: int, width: int, height: int },
}

fn area(s: Shape) -> int {
  match s {
    Rectangle { width, height, .. } => width * height,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_variant_construction() {
    let input = r#"
enum Message {
  Move { x: int, y: int },
}

fn make_move() -> Message {
  Move { x: 10, y: 20 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_same_variable_in_multiple_matches_option() {
    let input = r#"
fn test() -> int {
  let opt1: Option<int> = Some(5);
  let x1 = match opt1 {
    Some(v) => v,
    None => 0,
  };

  let opt2: Option<int> = Some(10);
  let x2 = match opt2 {
    Some(v) => v,
    None => 0,
  };

  x1 + x2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_same_variable_in_multiple_matches_result() {
    let input = r#"
fn test() -> int {
  let res1: Result<int, string> = Ok(5);
  let x1 = match res1 {
    Ok(v) => v,
    Err(e) => 0,
  };

  let res2: Result<int, string> = Ok(10);
  let x2 = match res2 {
    Ok(v) => v,
    Err(e) => 0,
  };

  x1 + x2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_same_variable_in_enum_with_fields() {
    let input = r#"
enum Event {
  Click(int, int),
  KeyPress(string),
}

fn test() -> int {
  let e1 = Event.Click(10, 20);
  let sum1 = match e1 {
    Event.Click(x, y) => x + y,
    Event.KeyPress(k) => 0,
  };

  let e2 = Event.Click(30, 40);
  let sum2 = match e2 {
    Event.Click(x, y) => x + y,
    Event.KeyPress(k) => 0,
  };

  sum1 + sum2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_shadowing_with_if_expression() {
    let input = r#"
fn test(cond: bool) -> int {
  let x = 1;
  let x = if cond { 2 } else { 3 };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_shadowing_with_match_expression() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let x = 1;
  let x = match opt {
    Some(v) => v,
    None => 0,
  };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_shadowing_multiple_times() {
    let input = r#"
fn test(cond1: bool, cond2: bool) -> int {
  let x = 1;
  let x = if cond1 { 2 } else { 3 };
  let x = if cond2 { x * 10 } else { x * 20 };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_shadowing_three_levels_method_call() {
    let input = r#"
fn test() -> int {
  let x = 10;
  let x = "now a string";
  let x = x.length();
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_shadowing_with_type_change() {
    let input = r#"
fn test() -> string {
  let x = 42;
  let x = "hello";
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_pattern_binding_same_name_as_subject() {
    let input = r#"
struct Big { a: int, b: int, c: int, d: int }

fn test() -> int {
  let b = Big { a: 1, b: 2, c: 3, d: 4 };
  match b {
    Big { a, b, .. } => a + b,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_self_enum_data_variant_uses_receiver_name() {
    let input = r#"
enum Shape {
  Circle(int),
  Rectangle { width: int, height: int },
}

impl Shape {
  fn area(self) -> int {
    match self {
      Shape.Circle(r) => r * r * 3,
      Shape.Rectangle { width, height } => width * height,
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_self_struct_field_pattern_uses_receiver_name() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn describe(self) -> string {
    match self {
      Point { x: 0, y: 0 } => "origin",
      Point { x: 0, y: _ } => "y-axis",
      Point { x: _, y: 0 } => "x-axis",
      _ => "other",
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_go_builtin_name_escaped_with_guards() {
    let input = r#"
fn categorize(items: Slice<int>) -> string {
  let len = items.length();
  match len {
    0 => "empty",
    n if n == 1 => "single",
    n if n <= 3 => "few",
    _ => "many",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn explicit_ref_enum_field_no_double_deref() {
    let input = r#"
enum List<T> {
  Cons(T, Ref<List<T>>),
  Nil,
}

fn list_len<T>(list: List<T>) -> int {
  match list {
    List.Nil => 0,
    List.Cons(_, rest) => 1 + list_len(rest.*),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_struct_variant_pattern() {
    let input = r#"
enum Tree<T> {
  Leaf(T),
  Node { left: Tree<T>, right: Tree<T> },
}

fn count_leaves<T>(tree: Tree<T>) -> int {
  match tree {
    Tree.Leaf(_) => 1,
    Tree.Node { left, right } => count_leaves(left) + count_leaves(right),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negative_integer_pattern_match() {
    let input = r#"
fn classify(x: int) -> string {
  match x {
    -100 => "very negative",
    -5 => "negative five",
    -1 => "negative one",
    0 => "zero",
    1 => "one",
    _ => "other",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn negative_pattern_i64_min_emit() {
    let input = r#"
fn classify(x: int) -> string {
  match x {
    -9223372036854775808 => "min",
    _ => "other",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn variable_shadow_inside_match_arm() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) => {
      let x = x * 2
      x
    },
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn multiple_shadows_in_block() {
    let input = r#"
fn test() -> int {
  let x = 5
  {
    let x = x * 2
    let x = x + 100
    x
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn variable_shadow_in_match_assigned_to_let() {
    let input = r#"
fn test() -> int {
  let val = Some(42)
  let result = match val {
    Some(x) => {
      let x = x * 2
      x
    },
    None => 0,
  }
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_one_arm_tuple_struct_reused_binding_name() {
    let input = r#"
struct Pair(int, int)
struct Name(string)

fn test() -> string {
  let p = Pair(1, 2);
  let a = match p {
    Pair(x, y) => x + y,
  };

  let n = Name("hello");
  let b = match n {
    Name(x) => x,
  };

  b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_one_arm_tuple_struct_no_outer_leak() {
    let input = r#"
struct Pair(int, int)

fn test() -> int {
  let a = 100;
  let b = 200;
  let p = Pair(1, 2);
  let sum = match p {
    Pair(a, b) => a + b,
  };
  a + b + sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_one_arm_tuple_no_outer_leak() {
    let input = r#"
fn test() -> string {
  let n = 50;
  let s = "original";
  let pair = (100, "replaced");
  let r = match pair {
    (n, s) => f"{n}-{s}",
  };
  f"{n} {s} {r}"
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_struct_pattern_match() {
    let input = r#"
struct UserId(int)

fn test(uid: UserId) -> int {
  match uid {
    UserId(id) => id,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pattern_unicode_escape_conversion() {
    let input = r#"
fn test(s: string) -> int {
  match s {
    "\u{00E9}" => 1,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_let_else_shadow() {
    let input = r#"
enum E { A(int), B(int), C }

fn test(e: E) -> int {
  let x = 1
  let A(x) | B(x) = e else { return 0; };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_let_else_binding_shadowing() {
    let input = r#"
fn maybe() -> Option<int> { Some(1) }

fn main() {
  let Some(x) | Some(x) = maybe() else { return };
  let _ = x;
  let x = 2;
  let _ = x;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_go_interface_emits_type_switch() {
    let input = r#"
import "go:example.com/events"

fn handle(e: events.Event) -> int {
  match e {
    events.Click { x, y } => x + y,
    events.KeyPress { key } => key.length(),
    _ => 0,
  }
}
"#;
    let typedef = r#"
pub interface Event {}
pub struct Click { pub x: int, pub y: int }
pub struct KeyPress { pub key: string }
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/events", typedef)]);
}

#[test]
fn match_on_go_interface_guarded_arm() {
    let input = r#"
import "go:example.com/events"

fn handle(e: events.Event, active: bool) -> int {
  match e {
    events.Click { x, y } => x + y,
    events.KeyPress { .. } if active => -1,
    _ => 0,
  }
}
"#;
    let typedef = r#"
pub interface Event {}
pub struct Click { pub x: int, pub y: int }
pub struct KeyPress { pub key: string }
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/events", typedef)]);
}
#[test]
fn match_on_go_interface_wildcard_only_arm() {
    let input = r#"
import "go:example.com/events"

fn handle(e: events.Event) -> int {
  match e {
    events.Click { x, y } => x + y,
    events.KeyPress { .. } => -1,
    events.Resize { .. } => 0,
    _ => 0,
  }
}
"#;
    let typedef = r#"
pub interface Event {}
pub struct Click { pub x: int, pub y: int }
pub struct KeyPress { pub key: string }
pub struct Resize { pub width: int, pub height: int }
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/events", typedef)]);
}

#[test]
fn match_on_aliased_go_interface_emits_type_switch() {
    let input = r#"
import "go:example.com/events"

fn handle(m: events.Msg) -> int {
  match m {
    events.Click { x, y } => x + y,
    events.KeyPress { key } => key.length(),
    _ => 0,
  }
}
"#;
    let typedef = r#"
pub interface Event {}
pub type Msg = Event
pub struct Click { pub x: int, pub y: int }
pub struct KeyPress { pub key: string }
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/events", typedef)]);
}

#[test]
fn match_on_aliased_lisette_enum_emits_enum_tag_switch() {
    let input = r#"
enum Color { Red, Green, Blue }

type Palette = Color

fn describe(p: Palette) -> string {
  match p {
    Color.Red => "r",
    Color.Green => "g",
    Color.Blue => "b",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_chained_alias_of_go_interface_emits_type_switch() {
    let input = r#"
import "go:example.com/events"

fn handle(m: events.Outer) -> int {
  match m {
    events.Click { x, y } => x + y,
    events.KeyPress { key } => key.length(),
    _ => 0,
  }
}
"#;
    let typedef = r#"
pub interface Event {}
pub type Inner = Event
pub type Outer = Inner
pub struct Click { pub x: int, pub y: int }
pub struct KeyPress { pub key: string }
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/events", typedef)]);
}
