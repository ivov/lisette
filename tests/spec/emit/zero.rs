use crate::assert_emit_snapshot;

#[test]
fn zero_of_type_parameter() {
    let input = r#"
fn empty<T: Zeroable>() -> T {
  zero<T>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zeroable_bound_renders_as_any() {
    let input = r#"
fn pair<T: Zeroable>(value: T) -> (T, T) {
  (value, zero<T>())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_primitives() {
    let input = r#"
fn f() {
  let n = zero<int>()
  let x = zero<float64>()
  let s = zero<string>()
  let b = zero<bool>()
  let _ = (n, x, s, b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_struct_and_option_and_slice() {
    let input = r#"
struct Point { x: int, y: int }

fn f() {
  let p = zero<Point>()
  let o = zero<Option<int>>()
  let s = zero<Slice<int>>()
  let _ = (p, o, s)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn a_local_binding_named_zero_is_called_not_zeroed() {
    // Calling a local named `zero` must not lower to a zero value.
    let input = r#"
fn f() -> int {
  let zero = || 42
  zero()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_unit_in_statement_position_is_dropped() {
    let input = r#"
fn f() -> int {
  let _ = zero<()>()
  1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_a_tuple_pins_its_element_types() {
    let input = r#"
fn take(t: (int, float64)) -> float64 { t.1 }

fn f() -> float64 {
  take(zero<(int, float64)>())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_an_enum_with_a_default_variant() {
    let input = r#"
enum Status { Active, Paused, #[default] Stopped }

fn f() -> Status {
  zero<Status>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_a_non_default_numeric_alias_carries_its_type() {
    let input = r#"
type Score = float64

fn f() -> Unknown {
  zero<Score>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_uintptr_carries_its_type() {
    let input = r#"
fn f() -> Unknown {
  zero<uintptr>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_in_argument_position_stays_an_expression() {
    let input = r#"
fn take(n: int) -> int { n }

fn f() -> int {
  take(zero<int>())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_type_parameter_in_argument_position() {
    let input = r#"
fn take<T: Zeroable>(value: T) -> T { value }

fn f<T: Zeroable>() -> T {
  take<T>(zero<T>())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_fills_a_json_target() {
    let input = r#"
import "go:encoding/json"

pub fn parse_json<T: Zeroable>(data: Slice<byte>) -> Result<T, error> {
  let mut result = zero<T>()
  json.Unmarshal(data, &result)?
  Ok(result)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_type_parameter_takes_a_selector() {
    let input = r#"
interface Speaker { fn Speak() -> string }

fn greet<T: Zeroable + Speaker>() -> string {
  zero<T>().Speak()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_a_non_default_numeric_carries_its_type() {
    let input = r#"
fn widened() -> Unknown {
  zero<float64>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_unit_is_a_value_not_a_statement() {
    let input = r#"
fn f() {
  let u = zero<()>()
  let _ = u
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_into_an_interface_slot_keeps_the_concrete_zero() {
    // A wider slot zeroes to nil, so the shortcut must not claim this one.
    let input = r#"
interface Speaker { fn speak() -> string }
struct Dog { name: string }
impl Dog { fn speak(self) -> string { "woof" } }

fn f() -> string {
  let s: Speaker = zero<Dog>()
  s.speak()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_into_an_unknown_slot_keeps_the_concrete_zero() {
    let input = r#"
struct Point { x: int, y: float64 }

fn f() {
  let n: Unknown = zero<int>()
  let s: Unknown = zero<string>()
  let b: Unknown = zero<bool>()
  let p: Unknown = zero<Point>()
  let _ = (n, s, b, p)
}
"#;
    assert_emit_snapshot!(input);
}
