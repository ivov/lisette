use crate::assert_emit_snapshot;

#[test]
fn array_let_destructure() {
    let input = r#"
fn f(arr: Array<int, 3>) -> int {
  let [a, b, c] = arr
  a + b + c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_match_destructure() {
    let input = r#"
fn f(arr: Array<int, 3>) -> int {
  match arr {
    [a, b, c] => a + b + c
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_rest_pattern_binds_sub_array() {
    let input = r#"
fn f(arr: Array<int, 3>) -> Array<int, 2> {
  let [_first, ..rest] = arr
  rest
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_let_else_rest_declares_sub_array() {
    let input = r#"
fn f(arr: Array<int, 3>) -> Array<int, 2> {
  let [0, ..rest] = arr else { return [9, 9] }
  rest
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_literal() {
    let input = r#"
fn test() -> Array<int, 3> {
  [1, 2, 3]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_type_in_signature() {
    let input = r#"
fn first(xs: Array<int, 3>) -> int {
  xs[0]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_struct_field() {
    let input = r#"
struct Grid {
  cells: Array<int, 4>,
}

fn make() -> Grid {
  Grid { cells: [1, 2, 3, 4] }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_length() {
    let input = r#"
fn count(xs: Array<int, 3>) -> int {
  xs.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_equality() {
    let input = r#"
fn same(a: Array<int, 2>, b: Array<int, 2>) -> bool {
  a == b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_array_type() {
    let input = r#"
fn grid() -> Array<Array<int, 3>, 2> {
  [[1, 2, 3], [4, 5, 6]]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn literal_elements_of_a_wider_type_keep_their_own_type() {
    let input = r#"
interface Shape {
  fn area() -> int
}

struct Circle {
  r: int
}

impl Circle {
  fn area(self) -> int { self.r }
}

fn test() -> int {
  let shapes: Slice<Shape> = [Circle { r: 1 }, Circle { r: 2 }]
  shapes[0].area() + shapes[1].area()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn element_calling_a_method_on_a_literal_keeps_its_type() {
    let input = r#"
struct Point {
  x: int,
  y: int
}

impl Point {
  fn scaled(self) -> Point { Point { x: self.x * 2, y: self.y * 2 } }
}

fn test() -> int {
  let pts: Slice<Point> = [Point { x: 1, y: 2 }.scaled(), Point { x: 3, y: 4 }]
  pts[0].x + pts[1].y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn element_slicing_a_literal_keeps_its_type() {
    let input = r#"
fn test() -> int {
  let rows: Slice<Slice<int>> = [[1, 2, 3][..2], [4, 5]]
  rows[0][0] + rows[1][1]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn element_spreading_a_call_result_keeps_its_type() {
    let input = r#"
struct Point {
  x: int,
  y: int
}

impl Point {
  fn scaled(self) -> Point { Point { x: self.x * 2, y: self.y * 2 } }
}

fn test() -> int {
  let pts: Slice<Point> = [Point { ..Point { x: 1, y: 2 }.scaled() }]
  pts[0].x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn element_spreading_a_literal_elides_its_type() {
    let input = r#"
struct Point {
  x: int,
  y: int
}

fn test() -> int {
  let pts: Slice<Point> = [Point { ..Point { x: 5, y: 6 } }]
  pts[0].x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_zero_value() {
    let input = r#"
struct Buf {
  data: Array<int, 4>,
}

fn empty() -> Buf {
  Buf { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_new_turbofish() {
    let input = r#"
fn make() -> Array<int, 5> {
  Array.new<int, 5>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_new_from_annotation() {
    let input = r#"
fn make() -> Array<int, 3> {
  let xs: Array<int, 3> = Array.new()
  xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_for_loop() {
    let input = r#"
fn sum(a: Array<int, 3>) -> int {
  let mut total = 0
  for x in a {
    total = total + x
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn zero_length_array() {
    let input = r#"
fn empty() -> Array<int, 0> {
  []
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_in_containers() {
    let input = r#"
type Addr = Array<byte, 4>

struct Holder {
  slice_of_arr: Slice<Array<int, 3>>,
  map_val_arr: Map<string, Array<int, 3>>,
  ptr_to_arr: Ref<Array<int, 3>>,
  multidim: Array<Array<int, 3>, 2>,
  aliased: Addr,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_as_map_key() {
    let input = r#"
fn count(m: Map<Array<int, 2>, string>) -> int {
  m.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_array_map_key_renders_comparable_bound() {
    let input = r#"
fn count<T: Comparable>(m: Map<Array<T, 2>, int>) -> int {
  m.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_over_array_map_key_renders_comparable_bound() {
    let input = r#"
type Key<T> = Array<T, 2>

fn f<T: Comparable>(m: Map<Key<T>, int>) -> int {
  m.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_key_with_array_field_renders_comparable_bound() {
    let input = r#"
struct Key<T> {
  value: Array<T, 2>,
}

fn f<T: Comparable>(m: Map<Key<T>, int>) -> int {
  m.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn phantom_generic_in_struct_key_stays_unbounded() {
    let input = r#"
struct Phantom<T> {
  n: int,
}

fn f<T>(m: Map<Phantom<T>, int>) -> int {
  m.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_array_map_key_uses_declared_comparable_bounds() {
    let input = r#"
struct Box<K: Comparable> {
  table: Map<K, int>,
}

fn f<T: Comparable>(b: Box<Array<T, 2>>) -> int {
  b.table.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_get_to_slice_identifier_form() {
    let input = r#"
fn at(xs: Array<int, 3>, i: int) -> Option<int> {
  Array.get(xs, i)
}

fn all(xs: Array<int, 3>) -> Slice<int> {
  Array.to_slice(xs)
}
"#;
    assert_emit_snapshot!(input);
}

// Zero values: primitive elements keep Go's `[N]T{}` zero-fill, but elements
// whose Lisette zero differs from Go's (e.g. `Option<T>`: None vs `Some(nil)`)
// must be filled per index.

#[test]
fn array_new_primitive_elements_use_go_zero_fill() {
    let input = r#"
fn ints() -> Array<int, 3> {
  Array.new<int, 3>()
}

fn bools() -> Array<bool, 2> {
  Array.new<bool, 2>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_new_option_element_fills_with_none() {
    let input = r#"
fn opts() -> Array<Option<int>, 2> {
  Array.new<Option<int>, 2>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_new_struct_with_option_field_fills() {
    let input = r#"
struct Horse {
  speed: int,
  fast: Option<int>,
}

fn herd() -> Array<Horse, 2> {
  Array.new<Horse, 2>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_zero_value_struct_field_with_option_element() {
    let input = r#"
struct Horse {
  speed: int,
  fast: Option<int>,
}

struct Stable {
  horses: Array<Horse, 2>,
}

fn empty() -> Stable {
  Stable { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_new_option_ref_element_fills_with_none() {
    let input = r#"
fn flags() -> Array<Option<Ref<bool>>, 2> {
  Array.new<Option<Ref<bool>>, 2>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_zero_value_zeroless_element_emits_empty_literal() {
    let input = r#"
struct S {
  refs: Array<Ref<int>, 0>,
  n: int,
}

fn make() -> S {
  S { n: 1, .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_copies_into_new_slice() {
    let input = r#"
fn to_slice(a: Array<int, 3>) -> Slice<int> {
  a.to_slice()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_get_returns_bounds_checked_option() {
    let input = r#"
fn at(a: Array<int, 3>, i: int) -> Option<int> {
  a.get(i)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_get_map_value_receiver_hoists() {
    let input = r#"
fn at(m: Map<string, Array<int, 3>>, i: int) -> Option<int> {
  m["k"].get(i)
}
"#;
    assert_emit_snapshot!(input);
}

// A `Ref` receiver is staged as `*a`, so the slice reads `(*a)[:]`.
#[test]
fn array_get_through_ref_deref() {
    let input = r#"
fn at(a: Ref<Array<int, 3>>, i: int) -> Option<int> {
  a.get(i)
}
"#;
    assert_emit_snapshot!(input);
}

// A transparent alias over `Array` must behave like the array at use sites:
// construction, indexing, and the prelude methods all lower natively, with no
// comma-ok double-wrap around `.get()`.
#[test]
fn array_methods_through_type_alias() {
    let input = r#"
type Addr = Array<byte, 4>

fn build() -> Addr {
  [1, 2, 3, 4]
}

fn at(a: Addr, i: int) -> Option<byte> {
  a.get(i)
}

fn to_slice(a: Addr) -> Slice<byte> {
  a.to_slice()
}

fn size(a: Addr) -> int {
  a.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_inequality() {
    let input = r#"
fn differ(a: Array<int, 2>, b: Array<int, 2>) -> bool {
  a != b
}
"#;
    assert_emit_snapshot!(input);
}

// Go arrays are value types, so a direct element assignment mutates the local
// copy; the assignment lowers to `b[0] = 9`.
#[test]
fn array_element_assignment() {
    let input = r#"
fn overwrite(a: Array<int, 3>) -> Array<int, 3> {
  let mut b = a
  b[0] = 9
  b
}
"#;
    assert_emit_snapshot!(input);
}

// Assignment through a `Ref` mutates the pointee, lowering to `(*a)[0] = 9`, so
// the caller observes the change.
#[test]
fn array_element_assignment_through_ref() {
    let input = r#"
fn bump(a: mut Ref<Array<int, 3>>) {
  a.*[0] = 9
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_size_from_constant() {
    let input = r#"
const SIZE = 3

struct Holder {
  pub data: Array<int, SIZE>
}

fn take(xs: Array<int, SIZE>) -> int {
  xs.length()
}

fn build() -> Holder {
  Holder { data: Array.new<int, SIZE>() }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_rest_pattern_with_func_elements() {
    let input = r#"
fn f(arr: Array<fn(int) -> (), 3>) -> Array<fn(int) -> (), 2> {
  let [_first, ..rest] = arr
  rest
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_skips_copy_for_reading_methods() {
    let input = r#"
fn reads(arr: Array<int, 3>) -> (bool, bool, Option<int>, int, bool) {
  let hit = arr.to_slice().contains(2)
  let same = arr.to_slice().equals([1, 2, 3])
  let second = arr.to_slice().get(1)
  let n = arr.to_slice().length()
  let blank = arr.to_slice().is_empty()
  (hit, same, second, n, blank)
}

fn copies(arr: Array<int, 3>, names: Array<string, 2>) -> (Slice<int>, string) {
  let copy = arr.to_slice().clone()
  let line = names.to_slice().join("-")
  (copy, line)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_skip_identifier_form() {
    let input = r#"
fn f(arr: Array<int, 3>) -> bool {
  Slice.contains(Array.to_slice(arr), 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_skip_negated_is_empty() {
    let input = r#"
fn f(arr: Array<int, 3>) -> bool {
  !arr.to_slice().is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_skip_through_alias_and_ref() {
    let input = r#"
type Addr = Array<byte, 4>

fn alias_receiver(a: Addr) -> bool {
  a.to_slice().contains(2)
}

fn ref_receiver(a: Ref<Array<int, 3>>) -> bool {
  a.to_slice().contains(2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_skip_hoists_unaddressable_receiver() {
    let input = r#"
fn make_arr() -> Array<int, 3> {
  [7, 8, 9]
}

fn f() -> bool {
  make_arr().to_slice().contains(8)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_keeps_copy_when_argument_calls() {
    let input = r#"
fn f() -> bool {
  let mut arr: Array<int, 3> = [1, 2, 3]
  let bump = || {
    arr[0] = 99
    return 99
  }
  arr.to_slice().contains(bump())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_keeps_copy_for_custom_equality() {
    let input = r#"
struct Point {
  x: int,
}

impl Point {
  fn equals(self, other: Point) -> bool {
    self.x == other.x
  }
}

fn user_equals(pts: Array<Point, 2>, want: Point) -> bool {
  pts.to_slice().contains(want)
}

fn nested_containers(rows: Array<Slice<int>, 2>, want: Slice<int>) -> bool {
  rows.to_slice().contains(want)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_keeps_copy_for_excluded_methods() {
    let input = r#"
fn f(arr: Array<int, 3>) -> (Slice<int>, int, int, Slice<int>, Slice<int>) {
  let doubled = arr.to_slice().map(|x| x * 2)
  let stomped = arr.to_slice().copy_from([7, 8])
  let room = arr.to_slice().capacity()
  let grown = arr.to_slice().reserve(10)
  let extended = arr.to_slice().append(4)
  (doubled, stomped, room, grown, extended)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_keeps_copy_when_deferred_directly() {
    let input = r#"
fn f() {
  let mut arr: Array<int, 3> = [1, 2, 3]
  defer arr.to_slice().contains(2)
  task arr.to_slice().contains(3)
  arr[0] = 99
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_to_slice_chain_skips_copy_in_async_arguments_and_blocks() {
    let input = r#"
fn consume(hit: bool) {
  let _ = hit
}

fn f() {
  let mut arr: Array<int, 3> = [1, 2, 3]
  defer consume(arr.to_slice().contains(2))
  task consume(arr.to_slice().contains(3))
  defer {
    consume(arr.to_slice().contains(2))
  }
  task {
    consume(arr.to_slice().contains(3))
  }
  arr[0] = 99
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_turbofish() {
    let input = r#"
fn make(xs: Slice<int>) -> Option<Array<int, 3>> {
  Array.from<int, 3>(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_from_annotation() {
    let input = r#"
fn make(xs: Slice<int>) -> Option<Array<int, 3>> {
  Array.from(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_zero_length() {
    let input = r#"
fn make(xs: Slice<int>) -> Option<Array<int, 0>> {
  Array.from(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_element_without_zero() {
    let input = r#"
enum Colour { Red, Green }

fn make(xs: Slice<Colour>) -> Option<Array<Colour, 2>> {
  Array.from(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_function_element_parenthesizes_the_conversion() {
    let input = r#"
fn make(xs: Slice<fn(int) -> int>) -> Option<Array<fn(int) -> int, 2>> {
  Array.from(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_option_element() {
    let input = r#"
fn make(xs: Slice<Option<int>>) -> Option<Array<Option<int>, 2>> {
  Array.from(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_in_match_binds_comma_ok_pair() {
    let input = r#"
fn first(xs: Slice<int>) -> int {
  match Array.from<int, 3>(xs) {
    Some(a) => a[0],
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_propagates_with_question_mark() {
    let input = r#"
fn first(xs: Slice<int>) -> Option<int> {
  let a = Array.from<int, 3>(xs)?
  Some(a[0])
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_stages_an_effectful_argument() {
    let input = r#"
fn grow(seen: mut Slice<int>) -> Slice<int> {
  seen.append(1)
}

fn f(seen: mut Slice<int>) -> Option<Array<int, 1>> {
  Array.from(grow(seen))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_named_constant_size() {
    let input = r#"
const SIZE = 3

fn make(xs: Slice<int>) -> Option<Array<int, SIZE>> {
  Array.from(xs)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn array_from_slice_alias_argument() {
    let input = r#"
type Buffer = Slice<byte>

fn head(b: Buffer) -> Option<Array<byte, 2>> {
  Array.from(b[0..2])
}
"#;
    assert_emit_snapshot!(input);
}
