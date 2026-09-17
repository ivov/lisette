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

#[test]
fn a_local_binding_named_zero_keeps_its_calls_in_statement_position() {
    // `zero<T>()` is droppable as a discarded statement; a binding that merely
    // shares the name is not, and dropping it would take its effects with it.
    let input = r#"
import "go:fmt"

fn f() -> int {
  let zero = || {
    fmt.Println("effect")
    42
  }
  let _ = zero()
  zero()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn autofill_of_a_zeroable_parameter_field_is_omitted() {
    let input = r#"
struct Box<T: Zeroable> { v: T, n: int }

fn blank<T: Zeroable>() -> Box<T> {
  Box { n: 1, .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_array_and_slice() {
    let input = r#"
fn f() {
  let a = zero<Array<int, 3>>()
  let b = zero<Array<string, 2>>()
  let s = zero<Slice<int>>()
  let _ = (a, b, s)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_a_wrapper_around_a_type_parameter() {
    let input = r#"
struct Box<T> { v: T, n: int }

fn blank<T: Zeroable>() -> Box<T> {
  zero<Box<T>>()
}

fn pair<T: Zeroable>() -> (T, int) {
  zero<(T, int)>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_embedded_and_tuple_structs() {
    let input = r#"
struct Base { x: int, tag: string }
struct Mid { embed Base, y: float64 }
struct Top { embed Mid, z: bool }
struct Meters(int)

fn nested() -> Top {
  zero<Top>()
}

fn newtype() -> Meters {
  zero<Meters>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_make_and_array_new_of_a_zeroable_parameter_keep_go_zero() {
    let input = r#"
fn buf<T: Zeroable>(n: int) -> Slice<T> {
  Slice.make<T>(n)
}

fn arr<T: Zeroable>() -> Array<T, 2> {
  Array.new<T, 2>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_unit_in_argument_and_tuple_positions_is_a_value() {
    let input = r#"
fn takes_unit(_u: ()) -> int { 1 }

fn f() -> int {
  let t = (zero<()>(), 2)
  takes_unit(zero<()>()) + t.1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn reference_to_zero_of_unit_is_a_value() {
    let input = r#"
fn f() -> Ref<()> {
  let p = &zero<()>()
  p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_of_enumerated_slice_is_nil_in_every_position() {
    let input = r#"
fn f() -> (EnumeratedSlice<int>, EnumeratedSlice<int>) {
  let xs = zero<EnumeratedSlice<int>>()
  (xs, zero<EnumeratedSlice<int>>())
}
"#;
    assert_emit_snapshot!(input);
}
