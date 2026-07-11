use crate::assert_emit_snapshot;
use crate::assert_emit_snapshot_with_go_typedefs;

#[test]
fn simple_struct_definition() {
    let input = r#"
struct Point { x: int, y: int }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_struct_field_emits_anonymously() {
    let input = r#"
pub struct Base { pub id: int }

struct Outer {
  embed Base,
  extra: int,
}

fn make() -> Outer {
  Outer { Base: Base { id: 1 }, extra: 2 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_pointer_field_emits_star_type() {
    let input = r#"
pub struct Base { pub id: int }

struct Outer {
  embed Ref<Base>,
}

fn make(b: Ref<Base>) -> Outer {
  Outer { Base: b }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_unexported_field_key_matches_type() {
    let input = r#"
struct inner { x: int }

struct Outer {
  embed inner,
}

fn make() -> Outer {
  Outer { inner: inner { x: 1 } }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_lowercase_alias_field_keeps_case() {
    let input = r#"
pub struct Base { pub id: int }
type p = Base

struct Outer {
  embed p,
}

fn read(o: Outer) -> int {
  o.p.id
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_field_untagged_under_struct_json() {
    let input = r#"
struct inner { x: int }

#[json]
struct Outer {
  embed inner,
}

fn read(o: Outer) -> int {
  o.inner.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn iterate_enum_variants() {
    let input = r#"
#[iterate]
pub enum BuildPhase {
  Validate,
  Parse,
  Codegen,
}

fn count() -> int {
  let mut total = 0
  for _phase in BuildPhase.variants() {
    total = total + 1
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_struct_synthesizes_to_string() {
    let input = r#"
#[display]
struct Point { x: int, y: int }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_enum_synthesizes_to_string() {
    let input = r#"
#[display]
enum Color { Red, Green }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_empty_struct_to_string() {
    let input = r#"
#[display]
struct Blank {}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_tuple_struct_to_string() {
    let input = r#"
#[display]
struct UserId(int)
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_generic_struct_to_string() {
    let input = r#"
#[display]
struct Box<T> { value: T }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_user_to_string_suppresses_synthesis() {
    let input = r#"
#[display]
struct Point { x: int, y: int }

impl Point {
  fn to_string(self) -> string {
    "p"
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_to_string_delegates_to_user_stringer() {
    let input = r#"
#[display]
struct Point { x: int, y: int }

impl Point {
  fn string(self) -> string {
    "p"
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_display_struct_gets_shadow_stringer() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

struct Server {
  embed Logger,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_display_shadow_is_transitive() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

struct Server {
  embed Logger,
  pub port: int,
}

struct Cluster {
  embed Server,
  pub name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_plain_struct_gets_no_shadow_stringer() {
    let input = r#"
struct Plain { pub v: int }

struct Wrapper {
  embed Plain,
  pub tag: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_outer_over_embedded_display_needs_no_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

#[display]
struct Server {
  embed Logger,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_user_stringer_promotes_without_shadow() {
    let input = r#"
struct Named { pub who: string }

impl Named {
  fn string(self) -> string {
    "Named!"
  }
}

struct Holder {
  embed Named,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_display_with_user_stringer_promotes_without_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

impl Logger {
  fn string(self) -> string {
    "custom"
  }
}

struct Server {
  embed Logger,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn deeper_promoted_stringer_does_not_block_shallower_display_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

struct N { pub n: int }

impl N {
  fn string(self) -> string {
    "N"
  }
}

struct P {
  embed N,
}

struct C {
  embed D,
  embed P,
  pub c: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_intermediate_user_stringer_stops_shadow_walk() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

struct Server {
  embed Logger,
  pub port: int,
}

impl Server {
  fn string(self) -> string {
    "srv"
  }
}

struct Cluster {
  embed Server,
  pub name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_alias_of_display_gets_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

type Alias = Logger

struct Server {
  embed Alias,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_display_pointer_gets_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

struct Server {
  embed Ref<Logger>,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_alias_to_ref_display_gets_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

type P = Ref<Logger>

struct Server {
  embed P,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_ref_to_alias_display_gets_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

type L = Logger

struct Server {
  embed Ref<L>,
  pub port: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn two_embedded_display_stringers_are_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct A { pub a: int }

#[display]
struct B { pub b: int }

struct C {
  embed A,
  embed B,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_display_and_user_stringer_are_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct A { pub a: int }

struct N { pub n: int }

impl N {
  fn string(self) -> string {
    "N!"
  }
}

struct C {
  embed A,
  embed N,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn single_embedded_display_beside_plain_embed_shadows() {
    let input = r#"
#[display]
struct A { pub a: int }

struct P { pub p: int }

struct C {
  embed A,
  embed P,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_interface_stringer_and_display_are_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

interface I {
  fn string(self) -> string
}

struct C {
  embed D,
  embed I,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_interface_parent_stringer_and_display_are_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

interface Base {
  fn string(self) -> string
}

interface I {
  embed Base
  fn area(self) -> int
}

struct C {
  embed D,
  embed I,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_interface_without_stringer_leaves_display_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

interface I {
  fn area(self) -> int
}

struct C {
  embed D,
  embed I,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_interface_non_stringer_string_method_is_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

interface I {
  fn string(self) -> int
}

struct C {
  embed D,
  embed I,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_generic_interface_stringer_and_display_are_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

interface Base<T> {
  fn string(self) -> T
}

struct C {
  embed D,
  embed Base<string>,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_generic_parent_interface_stringer_and_display_are_ambiguous_no_shadow() {
    let input = r#"
#[display]
struct D { pub x: int }

interface Base<T> {
  fn string(self) -> T
}

interface I {
  embed Base<string>
  fn area(self) -> int
}

struct C {
  embed D,
  embed I,
  pub n: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_field_beside_embedded_display_blocks_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

struct Server {
  embed Logger,
  pub string: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tag_exported_string_field_on_embed_blocks_outer_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

#[json]
struct Inner {
  embed Logger,
  string: int,
}

struct Outer {
  embed Inner,
  pub name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn bare_tag_does_not_force_export_so_outer_still_shadows() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

#[tag]
struct Inner {
  embed Logger,
  string: int,
}

struct Outer {
  embed Inner,
  pub name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn keyed_tag_forces_export_so_string_field_blocks_outer_shadow() {
    let input = r#"
#[display]
struct Logger { pub prefix: string }

#[tag("validate", "required")]
struct Inner {
  embed Logger,
  string: int,
}

struct Outer {
  embed Inner,
  pub name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_struct_comparable_fields() {
    let input = r#"
#[equality]
struct Point { x: int, y: int }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_struct_synthesizes_equals() {
    let input = r#"
#[equality]
struct Order { id: int, tags: Slice<string> }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_struct_ref_field_compares_by_identity() {
    let input = r#"
#[equality]
struct Node { value: int, next: Ref<Node> }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_struct_map_field() {
    let input = r#"
#[equality]
struct Counts { name: string, totals: Map<string, int> }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_struct_nested_equality_field() {
    let input = r#"
#[equality]
struct Inner { items: Slice<int> }

#[equality]
struct Outer { name: string, inner: Inner }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_slice_of_equality_struct_dispatches_equals() {
    let input = r#"
#[equality]
struct Order { id: int, tags: Slice<string> }

fn test(a: Slice<Order>, b: Slice<Order>) -> bool {
  a.equals(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_map_of_equality_struct_dispatches_equals() {
    let input = r#"
#[equality]
struct Order { id: int, tags: Slice<string> }

fn test(a: Map<string, Order>, b: Map<string, Order>) -> bool {
  a.equals(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_empty_struct() {
    let input = r#"
#[equality]
struct Blank {}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_generic_struct_comparable() {
    let input = r#"
#[equality]
struct Box<T: Comparable> { value: T }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_user_equals_suppresses_synthesis() {
    let input = r#"
#[equality]
struct Caseless { value: string }

impl Caseless {
  fn equals(self, other: Caseless) -> bool {
    self.value == other.value
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_enum_unit_variants() {
    let input = r#"
#[equality]
enum Color { Red, Green, Blue }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_enum_payload_variants() {
    let input = r#"
#[equality]
enum Shape {
  Circle(float64),
  Rectangle(float64, float64),
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_enum_mixed_variants() {
    let input = r#"
#[equality]
enum Message {
  Quit,
  Move { x: int, y: int },
  Write(string),
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_enum_slice_payload() {
    let input = r#"
#[equality]
enum Event {
  Tags(Slice<string>),
  Empty,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_generic_instantiation_compares_natively() {
    let input = r#"
struct Box<T: Comparable> {
  value: T,
}

fn same(a: Box<Box<int>>, b: Box<Box<int>>) -> bool {
  a == b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_recursive_enum_derefs_pointer_payload() {
    let input = r#"
#[equality]
enum List {
  Nil,
  Cons(int, List),
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_mutually_recursive_enums_deref_pointer_payloads() {
    let input = r#"
#[equality]
enum A {
  End,
  X(B),
}

#[equality]
enum B {
  Y(A),
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_indirect_recursive_enum_through_struct() {
    let input = r#"
#[equality]
enum Tree {
  Leaf,
  Node(Pair),
}

#[equality]
struct Pair {
  l: Tree,
  r: Tree,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_user_equals_over_function_field() {
    let input = r#"
#[equality]
struct Handler { run: fn(int) -> int }

impl Handler {
  fn equals(self, other: Handler) -> bool {
    true
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_matching_bound_generic_equals() {
    let input = r#"
#[equality]
struct Box<T: Comparable> { value: T }

impl<T: Comparable> Box<T> {
  fn equals(self, other: Box<T>) -> bool {
    self.value == other.value
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_weaker_method_bound_is_usable() {
    let input = r#"
#[equality]
struct Box<T: Ordered> { value: T }

impl<T: Comparable> Box<T> {
  fn equals(self, other: Box<T>) -> bool {
    self.value == other.value
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_weaker_interface_method_bound_is_usable() {
    let input = r#"
interface Parent {
  fn p(self)
}

interface Child {
  embed Parent

  fn c(self)
}

#[equality]
struct Box<T: Child> { value: T }

impl<T: Parent> Box<T> {
  fn equals(self, other: Box<T>) -> bool {
    true
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_field_private_cross_module_equals_uses_native() {
    let input = r#"
struct Foo { x: int }

impl Foo {
  fn equals(self, other: int) -> bool {
    false
  }
}

#[equality]
struct Wrap { foo: Foo }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_slice_of_generic_synthesized_equality() {
    let input = r#"
#[equality]
struct Box<T: Comparable> { items: Slice<T> }

fn test(a: Slice<Box<int>>, b: Slice<Box<int>>) -> bool {
  a.equals(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_nested_generic_synthesized_equality() {
    let input = r#"
#[equality]
struct Box<T: Comparable> { items: Slice<T> }

#[equality]
struct Wrap { box: Box<int> }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_enum_payload_generic_synthesized_equality() {
    let input = r#"
#[equality]
struct Box<T: Comparable> { items: Slice<T> }

#[equality]
enum Wrap { Wrapped(Box<int>) }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_renamed_generic_param_equals() {
    let input = r#"
#[equality]
struct Box<T: Comparable> { value: T }

impl<U: Comparable> Box<U> {
  fn equals(self, other: Box<U>) -> bool {
    self.value == other.value
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_equals_keeps_user_equals_live() {
    let input = r#"
struct Point { x: int }

impl Point {
  fn equals(self, other: Point) -> bool {
    self.x == other.x
  }
}

fn test(a: Slice<Point>, b: Slice<Point>) -> bool {
  a.equals(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_equals_keeps_user_equals_live() {
    let input = r#"
struct Point { x: int }

impl Point {
  fn equals(self, other: Point) -> bool {
    self.x == other.x
  }
}

fn test(a: Map<string, Point>, b: Map<string, Point>) -> bool {
  a.equals(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_synthesized_keeps_nested_user_equals_live() {
    let input = r#"
struct Inner { x: int }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    self.x == other.x
  }
}

#[equality]
struct Outer { inner: Inner }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn equality_synthesized_keeps_nested_slice_user_equals_live() {
    let input = r#"
struct Inner { x: int }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    self.x == other.x
  }
}

#[equality]
struct Outer { items: Slice<Inner> }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn display_satisfies_display_interface() {
    let input = r#"
interface Display {
  fn to_string(self) -> string
}

#[display]
struct Point { x: int, y: int }

fn render(d: Display) -> string {
  d.to_string()
}

fn use_it() -> string {
  render(Point { x: 1, y: 2 })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_instantiation() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> Point {
  Point { x: 10, y: 20 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_different_types() {
    let input = r#"
struct User { name: string, age: int, active: bool }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn empty_struct() {
    let input = r#"
struct Blank {}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct() {
    let input = r#"
struct Box<T> { value: T }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct_instantiation() {
    let input = r#"
struct Box<T> { value: T }

fn test() -> Box<int> {
  Box { value: 42 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn simple_enum() {
    let input = r#"
enum Color { Red, Green, Blue }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_construction_simple() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test() -> Color {
  Red
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_construction_with_data() {
    let input = r#"
fn test() -> Option<int> {
  Some(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_with_constrained_impl_make_functions() {
    let input = r#"
interface Displayable {
  fn display(self) -> string
}

enum Either<L, R> {
  Left(L),
  Right(R),
}

impl<L: Displayable, R: Displayable> Either<L, R> {
  fn display(self) -> string {
    match self {
      Either.Left(l) => {
        let s = l.display()
        f"Left({s})"
      },
      Either.Right(r) => {
        let s = r.display()
        f"Right({s})"
      },
    }
  }
}

struct Name { value: string }
impl Name { fn display(self) -> string { self.value } }

fn test() -> Either<Name, Name> {
  Either.Left(Name { value: "hello" })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn simple_type_alias() {
    let input = r#"
type UserId = int
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_with_generic() {
    let input = r#"
type IntBox = Box<int>

struct Box<T> { value: T }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn non_generic_alias_pins_unconstrained_type_param() {
    let input = r#"
struct Phantom<T, P> { pub value: T }

type IntPhantom = Phantom<int, string>

fn make() -> IntPhantom {
  IntPhantom { value: 7, .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_field_access() {
    let input = r#"
struct Point { pub x: int, pub y: int }

type Location = Point

fn get_x(loc: Location) -> int {
  loc.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_in_return_type() {
    let input = r#"
struct Point { pub x: int, pub y: int }

type Location = Point

fn origin() -> Location {
  Point { x: 0, y: 0 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_generic_in_param_and_return() {
    let input = r#"
type Numbers = Slice<int>

fn sum(nums: Numbers) -> int {
  let mut total = 0
  for n in nums {
    total += n
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_in_struct_field() {
    let input = r#"
struct Inner { pub val: int }

type Wrapper = Inner

struct Outer { pub item: Wrapper }

fn get_val(o: Outer) -> int {
  o.item.val
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_chained() {
    let input = r#"
struct Point { pub x: int, pub y: int }

type Location = Point
type GeoPoint = Location

fn origin() -> GeoPoint {
  Point { x: 0, y: 0 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_chained_slice_iter_and_index() {
    let input = r#"
type Numbers = Slice<int>
type MoreNumbers = Numbers

fn first(nums: MoreNumbers) -> int {
  nums[0]
}

fn sum(nums: MoreNumbers) -> int {
  let mut total = 0
  for n in nums {
    total += n
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_block_generic() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn new(value: T) -> Box<T> {
    Box { value: value }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_block_bare_self() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn sum(self) -> int {
    self.x + self.y
  }
}

fn main() {
  let p = Point { x: 10, y: 20 };
  let s = p.sum();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_block_bare_self_generic() {
    let input = r#"
struct Container<T> { value: T }

impl<T> Container<T> {
  fn get(self) -> T {
    self.value
  }
}

fn main() {
  let c = Container { value: 42 };
  let v = c.get();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_static_method_inferred() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn new(value: T) -> Box<T> {
    Box { value: value }
  }
}

fn main() -> int {
  let b: Box<int> = Box.new(42);
  b.value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_static_method_explicit_type_arg() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn new(value: T) -> Box<T> {
    Box { value: value }
  }
}

fn main() -> int {
  let b = Box.new<int>(42);
  b.value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_receiver_method_ufcs_explicit_type_arg() {
    let input = r#"
struct Box<T> { value: T }

impl<T> Box<T> {
  fn get(self) -> T {
    self.value
  }
}

fn main() -> int {
  let b = Box { value: 42 };
  Box.get<int>(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_fn_explicit_unknown_type_arg() {
    let input = r#"
fn foo<T>(args: T) -> T {
  args
}

fn test() {
  let result = foo<Unknown>("Lilian")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_variant_non_t_data_needs_type_arg() {
    let input = r#"
enum MyResult<T> {
  Ok(T),
  Fail(string),
}

fn main() {
  let ok: MyResult<int> = MyResult.Ok(42);
  let fail: MyResult<int> = MyResult.Fail("oops");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_type() {
    let input = r#"
fn test() {
  (42, "hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_destructuring() {
    let input = r#"
fn test() -> int {
  let pair = (10, 20);
  let (a, b) = pair;
  a + b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_pattern_in_function_param() {
    let input = r#"
fn sum((x, y): (int, int)) -> int {
  x + y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_return_annotation() {
    let input = r#"
fn get_pair() -> (int, string) {
  (42, "hello")
}

fn test() {
  let (x, y) = get_pair();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_generic_struct() {
    let input = r#"
struct Box<T> { value: T }

fn test() -> Box<Box<int>> {
  Box { value: Box { value: 42 } }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_simple() {
    let input = r#"
import "go:fmt"

interface Drawable {
  fn draw();
}

fn test() {
  fmt.Print("Interfaces work")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_with_methods() {
    let input = r#"
import "go:fmt"

interface Reader {
  fn read() -> int;
  fn close();
}

fn test() {
  fmt.Print("Interface with methods")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_with_generics() {
    let input = r#"
import "go:fmt"

interface Container<T> {
  fn get() -> T;
  fn set(value: T);
}

fn test() {
  fmt.Print("Generic interface")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_self_referential_fbound_erased() {
    let input = r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

struct Foo{}

impl Foo {
  fn clone(self) -> Foo { Foo{} }
}

fn squiggle<A: Cloner<B>, B>(_: A, _: B) {}

fn main() {
  squiggle(Foo{}, Foo{})
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_self_param_stripped() {
    let input = r#"
interface Greetable {
  fn greet(self) -> string;
  fn name(self) -> string;
  fn reset(self);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_with_variant_field() {
    let input = r#"
enum MyResult<T> { Success(T), Failure }

fn test() -> MyResult<int> {
  Success(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_with_multiple_variant_fields() {
    let input = r#"
enum Either<L, R> { Left(L), Right(R) }

fn test() -> Either<int, string> {
  Left(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_spread_update() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> Point {
  let p = Point { x: 1, y: 2 };
  Point { x: 10, ..p }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_spread_update_multiple_fields() {
    let input = r#"
struct Config { host: string, port: int, timeout: int }

fn test() -> Config {
  let default_config = Config { host: "localhost", port: 8080, timeout: 30 };
  Config { port: 9000, timeout: 60, ..default_config }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_spread_only() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> Point {
  let base = Point { x: 1, y: 2 };
  Point { ..base }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_spread_update_public_fields() {
    let input = r#"
struct Point { pub x: int, pub y: int }

fn test() -> Point {
  let p = Point { x: 1, y: 2 };
  Point { x: 10, ..p }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_spread_temp_var_no_collision() {
    let input = r#"
import "go:fmt"

struct Point { x: int, y: int }

fn main() {
  let p1 = Point { x: 1, y: 2 }
  let p2 = Point { x: 3, ..p1 }
  let copy_1 = 7
  fmt.Println(p2.x, copy_1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_spread_builds_fresh_literal() {
    let input = r#"
enum Cursor {
  Point { id: int, x: int },
  Area { id: int, w: int },
}

fn test() -> Cursor {
  let base = Cursor.Point { id: 7, x: 10 };
  Cursor.Area { w: 3, ..base }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_spread_all_fields_assigned_discards_base_call() {
    let input = r#"
import "go:fmt"

enum Cursor {
  Point { id: int, x: int },
  Area { id: int, w: int },
}

fn make_base() -> Cursor { fmt.Println("base"); Cursor.Point { id: 1, x: 2 } }

fn test() -> Cursor {
  Cursor.Area { id: 4, w: 3, ..make_base() }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_spread_all_fields_assigned_consumes_identifier_base() {
    let input = r#"
enum Cursor {
  Point { id: int, x: int },
  Area { id: int, w: int },
}

fn test() -> Cursor {
  let base = Cursor.Point { id: 1, x: 2 };
  Cursor.Area { id: 4, w: 3, ..base }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_spread_side_effecting_base_evaluated_once() {
    let input = r#"
import "go:fmt"

enum Cursor {
  Point { id: int, x: int },
  Area { id: int, w: int },
}

fn make_base() -> Cursor { fmt.Println("base"); Cursor.Point { id: 1, x: 2 } }

fn test() -> Cursor {
  Cursor.Area { w: 3, ..make_base() }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_spread_field_before_base_eval_order() {
    let input = r#"
import "go:fmt"

struct Point { x: int, y: int }

fn make_x() -> int { fmt.Println("x"); 1 }
fn make_base() -> Point { fmt.Println("base"); Point { x: 0, y: 0 } }

fn main() {
  let _ = Point { x: make_x(), ..make_base() }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn empty_struct_literal() {
    let input = r#"
struct Empty {}

fn test() -> Empty {
  Empty {}
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_type_definition() {
    let input = r#"
fn test(m: Map<string, int>) -> int {
  m["key"]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_exclusive() {
    let input = r#"
fn test(arr: Slice<int>) -> Slice<int> {
  arr[1..4]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_inclusive() {
    let input = r#"
fn test(arr: Slice<int>) -> Slice<int> {
  arr[1..=4]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_from() {
    let input = r#"
fn test(arr: Slice<int>) -> Slice<int> {
  arr[2..]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_to() {
    let input = r#"
fn test(arr: Slice<int>) -> Slice<int> {
  arr[..3]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_range_full() {
    let input = r#"
fn test(arr: Slice<int>) -> Slice<int> {
  arr[..]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn mutable_subslice_cloned() {
    let input = r#"
fn test(arr: Slice<int>) {
  let view = arr[1..4]
  let mut owned = arr[1..4].clone()
  owned[0] = 99
  let _ = view
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn mutable_subslice_through_alias_cloned() {
    let input = r#"
type Numbers = Slice<int>

fn test() {
  let xs: Numbers = [1, 2, 3]
  let mut part = xs[0..2].clone()
  part[0] = 99
  let _ = xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn immutable_subslice_through_alias_capacity_capped() {
    let input = r#"
type Numbers = Slice<int>

fn test() {
  let xs: Numbers = [1, 2, 3]
  let view = xs[0..2]
  let grown = view.append(9)
  let _ = grown
  let _ = xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_field_access() {
    let input = r#"
fn test() -> int {
  let r = 0..10;
  r.start + r.end
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_struct_literal() {
    let input = r#"
fn test() -> int {
  let r: Range<int> = Range{ start: 0, end: 10 };
  r.start + r.end
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn range_ref_field_access() {
    let input = r#"
fn get_start(r: Ref<Range<int>>) -> int {
  r.start
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_struct() {
    let input = r#"
/// A 2D point with x and y coordinates.
struct Point { x: int, y: int }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_struct_fields() {
    let input = r#"
/// A user in the system.
struct User {
  /// The user's display name.
  pub name: string,
  /// The user's age in years.
  age: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_enum() {
    let input = r#"
/// Represents the status of a task.
enum Status { Pending, Running, Complete }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_enum_variants() {
    let input = r#"
/// Result of a network operation.
enum NetworkResult {
  /// The operation succeeded.
  Success,
  /// The operation timed out.
  Timeout,
  /// The operation failed with an error code.
  Error(int),
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_type_alias() {
    let input = r#"
/// A unique identifier for a user.
type UserId = int
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_interface() {
    let input = r#"
/// Types that can be displayed as text.
interface Displayable {
  fn display() -> string;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_const() {
    let input = r#"
/// The maximum number of connections.
const MAX_CONNECTIONS: int = 100
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn doc_comment_on_impl_method() {
    let input = r#"
struct Counter { count: int }

impl Counter {
  /// Creates a new counter starting at zero.
  fn new() -> Counter {
    Counter { count: 0 }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_single_field_def() {
    let input = r#"
struct UserId(int)
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pointer_backed_newtype_emits_no_stringer() {
    let input = r#"
struct Handle(Ref<int>)
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_single_field_construction() {
    let input = r#"
struct UserId(int)

fn test() -> UserId {
  UserId(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_single_field_access() {
    let input = r#"
struct UserId(int)

fn test(id: UserId) -> int {
  id.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_field_autofill_casts() {
    let input = r#"
struct GameModeParam(string)

struct Inner { pub x: int }
struct Wrapper(Inner)

struct Name(string)
struct DoubleName(Name)

struct Command {
  pub gamemode: GameModeParam,
  pub wrapper: Wrapper,
  pub double: DoubleName,
}

fn register() -> Command {
  Command { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_field_autofill_uses_f_names() {
    let input = r#"
struct MP(int, string)

struct Holder {
  pub p: MP,
}

fn make() -> Holder {
  Holder { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_field_autofill_uses_constructor() {
    let input = r#"
struct Probe {
  pub pair: (int, string),
  pub nested: (Option<int>, (int, int)),
}

fn make() -> Probe {
  Probe { .. }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_multi_field_def() {
    let input = r#"
struct Point(int, int)
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_multi_field_construction() {
    let input = r#"
struct Point(int, int)

fn test() -> Point {
  Point(10, 20)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_multi_field_access() {
    let input = r#"
struct Point(int, int)

fn test(p: Point) -> int {
  p.0 + p.1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_generic_single_field() {
    let input = r#"
struct Wrapper<T>(T)

fn test() -> Wrapper<int> {
  Wrapper(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_generic_multi_field() {
    let input = r#"
struct Pair<A, B>(A, B)

fn test() -> Pair<int, string> {
  Pair(42, "hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_zero_field() {
    let input = r#"
struct Marker()

fn test() -> Marker {
  Marker()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_method_self_access() {
    let input = r#"
struct Point(int, int)

impl Point {
  fn x(self: Point) -> int {
    self.0
  }

  fn sum(self: Point) -> int {
    self.0 + self.1
  }
}

fn test() -> int {
  let p = Point(10, 20);
  p.sum()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_generic_method() {
    let input = r#"
struct Wrapper<T>(T)

impl<T> Wrapper<T> {
  fn unwrap(self: Wrapper<T>) -> T {
    self.0
  }
}

fn test() -> int {
  let w = Wrapper(42);
  w.unwrap()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_if_let() {
    let input = r#"
struct Point(int, int)

fn test(p: Option<Point>) -> int {
  if let Some(Point(x, y)) = p {
    x + y
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_match_guard() {
    let input = r#"
struct Point(int, int)

fn test(p: Point) -> int {
  match p {
    Point(x, y) if x > 0 => x + y,
    Point(_, y) => y,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_bound_method_call() {
    let input = r#"
import "go:fmt"

interface Stringer {
  fn string_value() -> string
}

fn print_it<T: Stringer>(x: T) {
  let s = x.string_value();
  fmt.Print(f"{s}\n");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_json_tag() {
    let input = r#"
#[json]
struct User {
  name: string,
  age: int,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_json_snake_case() {
    let input = r#"
#[json(snake_case)]
struct User {
  first_name: string,
  last_name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_json_omitempty() {
    let input = r#"
#[json]
struct User {
  #[json(omitempty)]
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_json_name_override() {
    let input = r#"
#[json]
struct User {
  #[json("userName")]
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_json_skip() {
    let input = r#"
#[json]
struct User {
  #[json(skip)]
  password: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_multiple_tags() {
    let input = r#"
#[json]
#[db]
struct User {
  #[json(omitempty)]
  #[db("user_name")]
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_tag_raw_mode() {
    let input = r#"
#[tag]
struct User {
  #[tag(`json:"name,omitempty" validate:"required"`)]
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_tag_structured_defaults() {
    let input = r#"
#[tag("bson", snake_case)]
struct User {
  userName: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_negated_omitempty() {
    let input = r#"
#[json(omitempty)]
struct User {
  #[json(!omitempty)]
  id: int,
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_snake_case_oauth() {
    let input = r#"
#[json(snake_case)]
struct Auth {
  OAuth2Token: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_tag_ordering() {
    let input = r#"
#[yaml]
#[json]
#[xml]
#[db]
struct User {
  #[yaml("user")]
  #[json("user")]
  #[xml("user")]
  #[db("user")]
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_option_omitempty() {
    let input = r#"
#[json(omitempty)]
struct User {
  name: string,
  email: Option<string>,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_tag_json_omitempty_option() {
    let input = r#"
struct User {
  #[tag("json", omitempty)]
  email: Option<string>,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_yaml_option_omitempty() {
    let input = r#"
#[yaml(omitempty)]
struct User {
  name: string,
  email: Option<string>,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_multiple_raw_tags() {
    let input = r#"
#[tag("validate")]
#[tag("custom")]
struct User {
  #[tag(`validate:"required"`)]
  #[tag(`custom:"foo"`)]
  name: string,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_with_option_alias_omitempty() {
    let input = r#"
type Maybe<T> = Option<T>

#[json(omitempty)]
struct User {
  name: Maybe<string>,
  email: Option<string>,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_alias_of_option_in_return_position() {
    let input = r#"
type Maybe<T> = Option<T>

fn test() -> Maybe<int> {
  Some(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn prelude_type_shadowing_struct_and_methods() {
    let input = r#"
struct Span {
  start: int,
  end: int,
}

impl Span {
  fn new(start: int, end: int) -> Span {
    Span { start: start, end: end }
  }

  fn contains(self: Span, val: int) -> bool {
    val >= self.start && val < self.end
  }

  fn len(self: Span) -> int {
    self.end - self.start
  }
}

fn test() -> int {
  let r = Span.new(0, 10);
  r.len()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_tree() {
    let input = r#"
enum Tree {
  Leaf(int),
  Node(Tree, Tree),
}

fn test() -> Tree {
  Tree.Node(Tree.Leaf(1), Tree.Leaf(2))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_function_vs_constructor() {
    let input = r#"
struct Point(int, int)

fn add_points(a: Point, b: Point) -> Point {
  let Point(x1, y1) = a;
  let Point(x2, y2) = b;
  Point(x1 + x2, y1 + y2)
}

fn test() -> Point {
  let p1 = Point(1, 2);
  let p2 = Point(3, 4);
  add_points(p1, p2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn single_field_tuple_struct_destructure() {
    let input = r#"
struct Wrapper(int)

fn test() -> int {
  let w = Wrapper(42);
  let Wrapper(val) = w;
  val
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_generic_enum_list() {
    let input = r#"
enum List<T> {
  Nil,
  Cons(T, List<T>)
}

fn test() -> List<int> {
  List.Cons(1, List.Cons(2, List.Nil))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_ref_argument_no_double_pointer() {
    let input = r#"
enum List {
  Cons(int, Ref<List>),
  Nil,
}

fn test() -> List {
  let nil = List.Nil;
  List.Cons(3, &nil)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_ref_value_no_double_pointer() {
    let input = r#"
enum Tree {
  Leaf(int),
  Node(Ref<Tree>, Ref<Tree>),
}

fn leaf(n: int) -> Ref<Tree> {
  let t = Tree.Leaf(n);
  &t
}

fn node(l: Ref<Tree>, r: Ref<Tree>) -> Ref<Tree> {
  let t = Tree.Node(l, r);
  &t
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_pattern_match_derefs_pointer() {
    let input = r#"
enum List {
  Cons(int, List),
  Nil,
}

fn list_sum(l: List) -> int {
  match l {
    List.Cons(head, tail) => head + list_sum(tail),
    List.Nil => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_indirect_through_struct() {
    let input = r#"
enum Tree {
  Leaf(int),
  Node(Pair),
}

struct Pair {
  l: Tree,
  r: Tree,
}

fn sum(t: Tree) -> int {
  match t {
    Tree.Leaf(n) => n,
    Tree.Node(p) => sum(p.l) + sum(p.r),
  }
}

fn test() -> Tree {
  Tree.Node(Pair { l: Tree.Leaf(1), r: Tree.Leaf(2) })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enums_mutual() {
    let input = r#"
enum A {
  End,
  X(B),
}

enum B {
  Y(A),
}

fn test() -> A {
  A.X(B.Y(A.End))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_indirect_through_mixed_generic_instantiations() {
    let input = r#"
enum Tree {
  Leaf,
  Node(Outer),
}

struct Outer {
  a: Box<int>,
  b: Box<Tree>,
}

struct Box<T> {
  value: T,
}

fn test() -> Tree {
  Tree.Node(Outer { a: Box { value: 1 }, b: Box { value: Tree.Leaf } })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recursive_enum_indirect_through_alias() {
    let input = r#"
enum Tree {
  Leaf(int),
  Node(P),
}

type P = Pair

struct Pair {
  l: Tree,
  r: Tree,
}

fn test() -> Tree {
  Tree.Node(Pair { l: Tree.Leaf(1), r: Tree.Leaf(2) })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pub_field_assignment_capitalizes() {
    let input = r#"
pub struct Counter {
  pub value: int,
}

fn test() {
  let mut c = Counter { value: 0 };
  c.value = 10
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_static_method_emits_type_params() {
    let input = r#"
struct Stack<T> { items: Slice<T> }

impl<T> Stack<T> {
  fn new() -> Stack<T> {
    Stack { items: [] }
  }
}

fn test() {
  let s: Stack<int> = Stack.new();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn method_call_on_enum_variant() {
    let input = r#"
enum Color { Red, Green, Blue }

impl Color {
  fn name(self) -> string {
    match self {
      Color.Red => "red",
      Color.Green => "green",
      Color.Blue => "blue",
    }
  }
}

fn test() -> string {
  Color.Red.name()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn void_fn_in_tuple_return_type() {
    let input = r#"
fn make_pair() -> (fn() -> int, fn()) {
  let get = || -> int { 42 };
  let inc = || { };
  (get, inc)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_static_method_non_inferred_type_arg() {
    let input = r#"
struct Container<T> {
  items: Slice<T>,
  label: string,
}

impl<T> Container<T> {
  fn new(label: string) -> Container<T> {
    Container { items: Slice.new<T>(), label }
  }
}

fn test() -> string {
  let c = Container.new<int>("numbers")
  c.label
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_type_alias_callable() {
    let input = r#"
type Transformer = fn(int) -> int

fn apply(t: Transformer, val: int) -> int {
  t(val)
}

fn double(x: int) -> int {
  x * 2
}

fn test() -> int {
  apply(double, 21)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_struct_variant_with_ref_field() {
    let input = r#"
enum Tree {
  Leaf(int),
  Node { value: int, left: Ref<Tree>, right: Ref<Tree> },
}

fn test() -> Tree {
  let l = Tree.Leaf(1);
  let r = Tree.Leaf(2);
  Tree.Node { value: 0, left: &l, right: &r }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ok_with_same_type_params() {
    let input = r#"
fn test() -> Result<string, string> {
  Ok("hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_tuple_variant_with_fn_type() {
    let input = r#"
enum Transform<T> {
  Identity,
  Apply(fn(T) -> T),
}

fn test() -> int {
  let t = Transform.Apply(|x: int| x + 1);
  match t {
    Transform.Identity => 0,
    Transform.Apply(f) => f(41),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_interface_constraint_with_type_args() {
    let input = r#"
interface Mapper<A, B> {
  fn map_value(self, a: A) -> B
}

fn apply_mapper<M: Mapper<int, string>>(m: M, val: int) -> string {
  m.map_value(val)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_bounded_generic_absorbs_ref_into_type_param() {
    let input = r#"
interface Mutable {
  fn mutate(self, val: string)
}

struct Box {
  content: string,
}

impl Box {
  fn mutate(self: Ref<Box>, val: string) {
    self.content = val
  }
}

fn apply_mutation<T: Mutable>(item: Ref<T>, val: string) {
  item.mutate(val)
}

fn test() {
  let mut b = Box { content: "original" }
  apply_mutation(&b, "changed")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_bounded_generic_explicit_deref() {
    let input = r#"
interface Greetable {
  fn greet(self) -> string
}

struct Person { name: string }

impl Person {
  fn greet(self) -> string { f"Hello, {self.name}" }
}

fn greet_ref<T: Greetable>(item: Ref<T>) -> string {
  item.*.greet()
}

fn test() {
  let p = Person { name: "Alice" }
  greet_ref(&p)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_receiver_camel_cases_public_snake_case_field() {
    let input = r#"
struct Foo {
  pub bar_baz: string,
}

impl Foo {
  fn show(self: Ref<Foo>) -> string {
    self.bar_baz
  }
}

fn test() {
  let mut foo = Foo { bar_baz: "quux" }
  let _ = foo.show()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_map() {
    let input = r#"
fn test(m: Ref<Map<string, int>>) {
  let _ = m
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_slice() {
    let input = r#"
fn test(s: Ref<Slice<int>>) {
  let _ = s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_channel() {
    let input = r#"
fn test(c: Ref<Channel<string>>) {
  let _ = c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_slice_auto_deref_native_method() {
    let input = r#"
fn main() {
  let s = [1, 2, 3]
  let r = &s
  let _ = r.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_slice_auto_deref_for_loop() {
    let input = r#"
fn main() {
  let s = [1, 2, 3]
  let r = &s
  for v in r {
    let _ = v
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_newtype_dot0_auto_deref() {
    let input = r#"
struct UserId(int)

fn main() {
  let id = UserId(5)
  let r = &id
  let _ = r.0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_newtype_dot0_assignment_auto_deref() {
    let input = r#"
struct UserId(int)

fn main() {
  let mut id = UserId(1)
  let r = &id
  r.* = UserId(2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_channel_auto_deref_select() {
    let input = r#"
fn main() {
  let ch = Channel.buffered<int>(1)
  ch.send(42)
  let r = &ch
  let _ = select {
    let Some(v) = r.receive() => v,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_nested_field_assignment() {
    let input = r#"
struct Inner { x: int }
struct Wrap(Inner)

fn main() {
  let mut w = Wrap(Inner { x: 1 })
  let mut inner = w.0
  inner.x = 2
  w = Wrap(inner)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_slice_append() {
    let input = r#"
struct Wrap(Slice<int>)

fn main() {
  let mut w = Wrap([1])
  w = Wrap(w.0.append(2))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn double_newtype_nested_field_assignment() {
    let input = r#"
struct Inner { x: int }
struct A(Inner)
struct B(A)

fn main() {
  let mut w = B(A(Inner { x: 1 }))
  let mut inner = w.0.0
  inner.x = 2
  w = B(A(inner))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn double_newtype_slice_append() {
    let input = r#"
struct A(Slice<int>)
struct B(A)

fn main() {
  let mut w = B(A([]))
  w = B(A(w.0.0.append(1)))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn double_newtype_direct_assignment() {
    let input = r#"
struct A(int)
struct B(A)

fn main() {
  let mut w = B(A(1))
  w = B(A(2))
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_concrete_generic_method() {
    let input = r#"
import "go:fmt"

type MyInt = int
type MyString = string

enum Either<L, R> {
  Left(L),
  Right(R),
}

impl Either<MyInt, MyString> {
  fn describe(self) -> MyString {
    match self {
      Either.Left(n) => fmt.Sprintf("int(%d)", n),
      Either.Right(s) => fmt.Sprintf("str(%s)", s),
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_concrete_generic_call() {
    let input = r#"
import "go:fmt"

type MyInt = int
type MyString = string

enum Either<L, R> {
  Left(L),
  Right(R),
}

impl Either<MyInt, MyString> {
  fn describe(self) -> MyString {
    match self {
      Either.Left(n) => fmt.Sprintf("int(%d)", n),
      Either.Right(s) => fmt.Sprintf("str(%s)", s),
    }
  }
}

fn test() -> MyString {
  let e: Either<MyInt, MyString> = Either.Left(42)
  e.describe()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_method_pattern_binding_no_receiver_shadow() {
    let input = r#"
struct Node {
  pub value: int,
  pub next: Option<int>,
}

impl Node {
  fn sum(self) -> int {
    match self.next {
      Some(n) => self.value + n,
      None => self.value,
    }
  }
}

fn test() -> int {
  let node = Node { value: 10, next: Some(5) }
  node.sum()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pub_interface_methods_exported() {
    let input = r#"
pub interface Describable {
  fn describe(self) -> string
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn private_interface_methods_unexported() {
    let input = r#"
interface Describable {
  fn describe(self) -> string
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn pub_interface_impl_method_capitalized() {
    let input = r#"
pub interface Printable {
  fn display(self) -> string
}

struct Box {
  value: string,
}

impl Box {
  fn display(self) -> string {
    self.value
  }
}

fn describe(p: Printable) -> string {
  p.display()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ref_of_ref_collapses_in_codegen() {
    let input = r#"
fn test() {
  let mut x = 42
  let r1 = &x
  let r2 = &r1
  let val = r2.*
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn mutable_interface_var_uses_var_declaration() {
    let input = r#"
pub interface Printable {
  fn to_string(self) -> string
}

struct Box { label: string }
impl Box {
  pub fn to_string(self) -> string { self.label }
}

fn test() {
  let p: Printable = Box { label: "hello" }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn typed_let_loop_interface_declares_annotated_type() {
    let input = r#"
interface Printable {
  fn text(self) -> string
}

struct A {}
struct B {}

impl A { fn text(self) -> string { "a" } }
impl B { fn text(self) -> string { "b" } }

fn test() {
  let mut p: Printable = loop {
    break A {}
  }
  p = B {}
  let _ = p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn typed_let_propagate_interface_declares_annotated_type() {
    let input = r#"
interface Printable {
  fn text(self) -> string
}

struct A {}
struct B {}

impl A { fn text(self) -> string { "a" } }
impl B { fn text(self) -> string { "b" } }

fn make_a() -> Result<A, string> { Ok(A {}) }

fn run() -> Result<(), string> {
  let mut p: Printable = make_a()?
  p = B {}
  let _ = p
  Ok(())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn json_tagged_struct_field_access() {
    let input = r#"
#[json]
struct User {
  name: string,
  age: int,
}

fn test() -> string {
  let u = User { name: "Alice", age: 30 };
  u.name
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn assoc_fn_returning_option_self() {
    let input = r#"
struct Thing {
  name: string,
}

impl Thing {
  fn maybe_create(name: string) -> Option<Thing> {
    if name.length() > 0 { Some(Thing { name: name }) }
    else { None }
  }
}

fn test() -> Option<Thing> {
  Thing.maybe_create("hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_static_method_return_only_type_param() {
    let input = r#"
struct Container<T> {
  items: Slice<T>,
  name: string,
}

impl<T> Container<T> {
  fn new(name: string) -> Container<T> {
    Container { items: [], name: name }
  }
}

fn test() -> Container<string> {
  Container.new("things")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn function_type_alias() {
    let input = r#"
type IntPredicate = fn(int) -> bool

fn apply(f: IntPredicate, x: int) -> bool {
  f(x)
}

fn test() -> bool {
  apply(|x| x > 0, 42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn json_enum_simple_no_payload() {
    let input = r#"
#[json]
enum Status { Active, Inactive, Suspended }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn json_enum_with_single_payload() {
    let input = r#"
#[json]
enum Shape {
  Circle(float64),
  Square(float64),
  Unknown,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn json_enum_with_multi_payload() {
    let input = r#"
#[json]
enum Shape {
  Rectangle(float64, float64),
  Point,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn json_enum_with_struct_variant() {
    let input = r#"
#[json]
enum Message {
  Move { x: int, y: int },
  Quit,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_marshal_json_method_without_json_attribute() {
    let input = r#"
enum Status {
  Ready,
}

impl Status {
  fn MarshalJSON(self) -> Result<Slice<byte>, error> {
    Ok([])
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_return_only_type_param_string_vs_int() {
    let input = r#"
enum Validated<T> { Valid(T), Invalid(string) }

struct ValidationResult<T> {
  value: Validated<T>,
  field_name: string,
}

impl<T> ValidationResult<T> {
  fn new_invalid(field: string, msg: string) -> ValidationResult<T> {
    ValidationResult { value: Validated.Invalid(msg), field_name: field }
  }
}

fn validate_positive(field: string, val: int) -> ValidationResult<int> {
  ValidationResult.new_invalid(field, "must be positive")
}

fn validate_non_empty(field: string, val: string) -> ValidationResult<string> {
  ValidationResult.new_invalid(field, "must not be empty")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_function_type_alias() {
    let input = r#"
type Transform<T> = fn(T) -> T

fn apply_transform<T>(val: T, f: Transform<T>) -> T {
  f(val)
}

fn test() -> int {
  apply_transform(42, |x| x + 1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn user_defined_unit_enum() {
    let input = r#"
enum Tag { A, B, C }

fn test() -> bool {
  let u1 = Tag.A
  let u2 = Tag.B
  u1 == u2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn constrained_impl_block_emits_bound() {
    let input = r#"
interface Displayable {
  fn display(self) -> string
}

struct Labeled<T> {
  label: string,
  value: T,
}

impl<T: Displayable> Labeled<T> {
  fn show(self) -> string {
    self.label
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn specialized_impl_uses_ufcs() {
    let input = r#"
type MyInt = int

struct Box<T> { val: T }

impl Box<MyInt> {
  fn doubled(self) -> MyInt { self.val * 2 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn specialized_impl_builtin_type_string() {
    let input = r#"
struct Wrapper<T> {
  value: T,
}

impl Wrapper<string> {
  fn greet(self) -> string {
    "hello"
  }
}

fn test() -> string {
  let w = Wrapper { value: "world" };
  w.greet()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn specialized_impl_builtin_type_int() {
    let input = r#"
struct Box<T> {
  item: T,
}

impl Box<int> {
  fn unwrap(self) -> int {
    self.item
  }
}

fn test() -> int {
  let b = Box { item: 42 };
  b.unwrap()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_repeated_type_var_uses_ufcs() {
    let input = r#"
struct Pair<A, B> {
  a: A,
  b: B,
}

impl<T> Pair<T, T> {
  fn first(self) -> T {
    self.a
  }
}

fn test() -> int {
  let p = Pair { a: 1, b: 2 };
  p.first()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_primitive_arg_uses_ufcs() {
    let input = r#"
struct Pair<A, B> {
  a: A,
  b: B,
}

impl<B> Pair<int, B> {
  fn left(self) -> int {
    self.a
  }
}

fn test() -> int {
  let p = Pair { a: 1, b: "x" };
  p.left()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_nominal_arg_uses_ufcs() {
    let input = r#"
struct User {
  name: string,
}

struct Pair<A, B> {
  a: A,
  b: B,
}

impl<B> Pair<User, B> {
  fn left(self) -> User {
    self.a
  }
}

fn test() -> string {
  let p = Pair { a: User { name: "x" }, b: 1 };
  p.left().name
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn specialized_impl_explicit_type_args() {
    let input = r#"
struct Box<T> { value: T }

impl Box<int> {
  fn keep<U>(self, x: U) -> U { x }
}

fn test() -> string {
  let b = Box { value: 1 };
  b.keep<string>("ok")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_repeated_explicit_type_args() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<T> Pair<T, T> {
  fn keep<U>(self, x: U) -> U { x }
}

fn test() -> string {
  let p = Pair { a: 1, b: 2 };
  p.keep<int, string>("ok")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn specialized_impl_return_only_type_args() {
    let input = r#"
struct Box<T> { value: T }

impl Box<int> {
  fn make<U>(self) -> U { panic("boom") }
}

fn test() {
  let b = Box { value: 1 };
  let x: string = b.make();
  let _ = x;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_repeated_return_only_type_args() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<T> Pair<T, T> {
  fn make<U>(self) -> U { panic("boom") }
}

fn test() {
  let p = Pair { a: 1, b: 2 };
  let x: string = p.make();
  let _ = x;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_primitive_receiver_return_only_type_args() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<B> Pair<int, B> {
  fn make<U>(self) -> U { panic("boom") }
}

fn test() {
  let p: Pair<int, bool> = Pair { a: 1, b: true };
  let x: string = p.make();
  let _ = x;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_partial_impl_ufcs_call() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<T> Pair<T, T> {
  fn first(self) -> T { self.a }
}

type IntPair = Pair<int, int>

fn test() -> int {
  let p: IntPair = Pair { a: 1, b: 2 };
  p.first()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_specialized_impl_ufcs_call() {
    let input = r#"
struct Box<T> { value: T }

impl Box<int> {
  fn only_int(self) -> int { self.value }
}

type IntBox = Box<int>

fn test() -> int {
  let b: IntBox = Box { value: 7 };
  b.only_int()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn specialized_impl_phantom_type_arg() {
    let input = r#"
struct Box<T> { value: T }

impl Box<int> {
  fn tag<U>(self) -> int { 1 }
}

fn test() -> int {
  let b = Box { value: 1 };
  b.tag<string>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_phantom_type_arg() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<T> Pair<T, T> {
  fn tag<U>(self) -> int { 1 }
}

fn test() -> int {
  let p = Pair { a: 1, b: 2 };
  p.tag<string>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_impl_method_only_explicit_type_arg() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<T> Pair<T, T> {
  fn keep<U>(self, x: U) -> U { x }
}

fn test() -> string {
  let p = Pair { a: 1, b: 2 };
  p.keep<string>("ok")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn full_arity_type_args_with_nonreceiver_impl_generic() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<X, T> Pair<T, T> {
  fn pick(self, x: X) -> X { x }
}

fn test() -> bool {
  let p = Pair { a: 1, b: 2 };
  p.pick<bool, int>(true)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn multi_impl_generic_method_only_type_arg() {
    let input = r#"
struct Pair<A, B> { a: A, b: B }

impl<A, B> Pair<A, B> {
  fn snd<U>(self, u: U) -> U { u }
}

fn test() -> bool {
  let p: Pair<int, string> = Pair { a: 1, b: "x" };
  p.snd<bool>(true)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_named_string() {
    let input = r#"
enum ASTNode {
  Number(int),
  String(string),
  Null,
}

fn visit(node: ASTNode) -> string {
  match node {
    ASTNode.Number(n) => f"number:{n}",
    ASTNode.String(s) => f"string:{s}",
    ASTNode.Null => "null",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn err_with_errors_new() {
    let input = r#"
import "go:errors"

fn might_fail(n: int) -> Result<int, error> {
  if n < 0 {
    return Err(errors.New("negative"))
  }
  Ok(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn single_variant_unit_enum() {
    let input = r#"
enum Marker {
  Value,
}

fn test() -> Marker {
  Marker.Value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_with_map_key_type_parameter() {
    let input = r#"
enum Container<K, V> {
  Empty,
  Indexed(Map<K, V>),
}

fn test() {
  let mut m = Map.new<string, int>()
  m["hello"] = 1
  let c = Container.Indexed<string, int>(m)
  c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_enum_unit_variant() {
    let input = r#"
enum Color {
  Red,
  Green,
  Blue,
}

type MyColor = Color

fn test() -> MyColor {
  MyColor.Red
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_map_comparable_constraint() {
    let input = r#"
type KeyVal<K, V> = Map<K, V>

fn test() -> KeyVal<string, int> {
  let mut m = Map.new<string, int>()
  m["a"] = 1
  m
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_map_key_comparable_constraint() {
    let input = r#"
interface Store<K, V> {
  fn entries(self) -> Map<K, V>
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn newtype_field_assignment() {
    let input = r#"
import "go:fmt"

struct New(int)

fn main() {
  let mut n = New(1)
  n = New(2)
  fmt.Println(n.0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interface_private_method_keyword_escape() {
    let input = r#"
import "go:fmt"

interface Worker {
  fn range(self) -> int
}

struct S {
  value: int,
}

impl S {
  fn range(self) -> int { self.value }
}

fn main() {
  let s = S { value: 3 }
  let w: Worker = s
  fmt.Println(w.range())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_tuple_struct_constructor_type_args() {
    let input = r#"
struct Wrapper<T>(T)

fn main() {
  let w = Wrapper<int>(1)
  let z = w.0 + 1
  let _ = z
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_struct_static_method() {
    let input = r#"
struct Box { v: int }

impl Box {
  fn new(v: int) -> Box { Box { v } }
}

type Alias = Box

fn main() {
  let b = Alias.new(1)
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_native_ufcs_method() {
    let input = r#"
import "go:fmt"

type MyString = string

fn main() {
  let n = MyString.length("hi")
  fmt.Println(n)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn type_alias_tuple_struct_constructor() {
    let input = r#"
import "go:fmt"

struct Wrap(int)
type Alias = Wrap

fn main() {
  let v = Alias(1)
  fmt.Println(v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_interface_comparable_propagation() {
    let input = r#"
interface A<K> {
  fn get(self, m: Map<K, int>) -> int
}

interface B<K> {
  embed A<K>
}

fn main() { let _ = 0 }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn embedded_interface_comparable_transitive() {
    let input = r#"
interface A<K> {
  fn get(self, m: Map<K, int>) -> int
}

interface B<K> {
  embed A<K>
}

interface C<K> {
  embed B<K>
}

fn main() { let _ = 0 }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_constructor_value_type_args() {
    let input = r#"
enum Wrap<T> { W(T) }

fn main() {
  let f = Wrap.W
  let w = f(1)
  let _ = w
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn user_fn_option_tuple_return_call() {
    let input = r#"
fn maybe_pair(n: int) -> Option<(int, int)> {
  if n > 0 { Some((n, n + 1)) } else { None }
}
fn main() {
  let r = maybe_pair(5)
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn prelude_some_ok_constructor_value() {
    let input = r#"
fn main() {
  let f = Some
  let x = f(42)
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_struct_variant_named_tag() {
    let input = r#"
enum E {
  Tag { x: int },
  Other { x: int },
}

fn main() {
  let e = E.Tag { x: 1 }
  let _ = e
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_user_string_method_emits_go_string() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  pub fn string(self) -> string {
    "custom"
  }
}

fn main() {
  let p = Point { x: 1, y: 2 }
  let _ = p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_user_string_method_emits_go_string() {
    let input = r#"
struct UserId(int)

impl UserId {
  pub fn string(self) -> string {
    "custom"
  }
}

fn main() {
  let u = UserId(1)
  let _ = u
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_user_string_method_emits_go_string() {
    let input = r#"
enum Color {
  Red,
  Blue,
}

impl Color {
  pub fn string(self) -> string {
    "custom"
  }
}

fn main() {
  let c = Color.Red
  let _ = c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_user_string_and_go_string_methods() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  pub fn string(self) -> string {
    "custom display"
  }

  pub fn goString(self) -> string {
    "custom debug"
  }
}

fn main() {
  let p = Point { x: 1, y: 2 }
  let _ = p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_user_string_and_go_string_methods() {
    let input = r#"
struct UserId(int)

impl UserId {
  pub fn string(self) -> string {
    "custom display"
  }

  pub fn goString(self) -> string {
    "custom debug"
  }
}

fn main() {
  let u = UserId(1)
  let _ = u
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_user_string_and_go_string_methods() {
    let input = r#"
enum Color {
  Red,
  Blue,
}

impl Color {
  pub fn string(self) -> string {
    "custom display"
  }

  pub fn goString(self) -> string {
    "custom debug"
  }
}

fn main() {
  let c = Color.Red
  let _ = c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_user_pascal_string_method_suppresses_auto_stringer() {
    let input = r#"
struct A { a: string }

impl A {
  fn String(self) -> string {
    self.a + "asdf"
  }
}

fn main() {
  let a = A { a: "text" }
  let _ = a
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_struct_user_pascal_string_method_suppresses_auto_stringer() {
    let input = r#"
struct UserId(int)

impl UserId {
  fn String(self) -> string {
    "custom"
  }
}

fn main() {
  let u = UserId(1)
  let _ = u
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_user_pascal_string_method_suppresses_auto_stringer() {
    let input = r#"
enum Color {
  Red,
  Blue,
}

impl Color {
  fn String(self) -> string {
    "custom"
  }
}

fn main() {
  let c = Color.Red
  let _ = c
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ufcs_stringer_on_specialized_impl_does_not_suppress_auto() {
    let input = r#"
struct Box<T> { v: T }

impl Box<int> {
  fn String(self) -> string {
    "int_box"
  }
}

fn main() {
  let _ = Box { v: 42 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn struct_user_pascal_string_and_go_string_methods() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn String(self) -> string {
    "custom display"
  }

  fn GoString(self) -> string {
    "custom debug"
  }
}

fn main() {
  let p = Point { x: 1, y: 2 }
  let _ = p
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_methods_share_local_variable_name() {
    let input = r#"
pub struct Calc {}
impl Calc {
  pub fn wider(self) -> uint64 {
    let mut total: uint64 = 0
    total += 1
    total
  }

  pub fn narrower(self) -> uint32 {
    let mut total: uint32 = 0
    total += 1
    total
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unknown_return_in_struct_field_emits_any() {
    let input = r#"
struct Bag {
  pub callback: fn() -> Unknown,
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unknown_return_in_callback_param_emits_any() {
    let input = r#"
fn run(callback: fn() -> Unknown) {}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unknown_return_in_alias_emits_any() {
    let input = r#"
type Cb = fn() -> Unknown;
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn option_in_go_unknown_struct_field_stays_tagged() {
    let input = r#"
import "go:example.com/dyn"

fn main() {
  let _ = dyn.Bag {
    Decl: Some("decl"),
    Data: None,
  }
}
"#;
    let typedef = r#"
pub struct Bag {
  pub Decl: Unknown,
  pub Data: Unknown,
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/dyn", typedef)]);
}

#[test]
fn option_assigned_to_go_unknown_field_stays_tagged() {
    let input = r#"
import "go:example.com/dyn"

fn main() {
  let mut obj = dyn.Bag { Decl: "", Data: None }
  obj.Decl = None
  let _ = obj
}
"#;
    let typedef = r#"
pub struct Bag {
  pub Decl: Unknown,
  pub Data: Unknown,
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/dyn", typedef)]);
}

#[test]
fn option_in_go_unknown_call_arg_stays_tagged() {
    let input = r#"
import "go:example.com/dyn"

fn main() {
  let _ = dyn.Store("k", None)
  let _ = dyn.Store("k", Some(1))
}
"#;
    let typedef = r#"
pub fn Store(key: string, value: Unknown) -> int
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/dyn", typedef)]);
}

#[test]
fn option_in_go_generic_slots_stays_tagged() {
    let input = r#"
import "go:example.com/generic"

fn main() {
  let option = Some(1)
  let options: Slice<Option<int>> = [option]
  generic.Store(option)
  generic.StoreAll(options)
  generic.StoreMany(option, option)
  let loaded = generic.Load(option)
  let loaded_all = generic.LoadAll(options)
  let bag = generic.Bag { Value: option, Values: options }
  generic.Store(bag.Value)
  generic.Store(loaded)
  generic.StoreAll(loaded_all)
}
"#;
    let typedef = r#"
pub fn Store<T>(value: T)
pub fn StoreAll<T>(values: Slice<T>)
pub fn StoreMany<T>(values: VarArgs<T>)
pub fn Load<T>(value: T) -> T
pub fn LoadAll<T>(values: Slice<T>) -> Slice<T>

pub struct Bag<T> {
  pub Value: T,
  pub Values: Slice<T>,
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/generic", typedef)]);
}

#[test]
fn omitted_return_in_callback_param_stays_void() {
    let input = r#"
fn run(callback: fn(int)) {}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_method_map_key_return_constrains_struct_receiver() {
    let input = r#"
struct Box<T> {
  value: T,
}

impl<T> Box<T> {
  fn make_map(self) -> Map<T, int> {
    Map.new<T, int>()
  }
}

fn main() {
  let b = Box { value: "a" }
  let _ = b.make_map()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_method_map_key_body_constrains_struct_receiver() {
    let input = r#"
struct Box<T> {
  value: T,
}

impl<T> Box<T> {
  fn count(self) -> int {
    let m = Map.new<T, int>()
    m.length()
  }
}

fn main() {
  let b = Box { value: "a" }
  let _ = b.count()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impl_method_map_key_body_constrains_enum_receiver_and_make_function() {
    let input = r#"
enum Holder<T> {
  Empty,
}

impl<T> Holder<T> {
  fn count(self) -> int {
    let m = Map.new<T, int>()
    m.length()
  }
}

fn main() {
  let h: Holder<string> = Holder.Empty
  let _ = h.count()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_comparable_bound_lowers_to_go_comparable() {
    let input = r#"
fn id<T: Comparable>(x: T) -> T {
  x
}

fn main() {
  let _ = id(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_ordered_bound_lowers_to_cmp_ordered_with_import() {
    let input = r#"
fn id<T: Ordered>(x: T) -> T {
  x
}

fn main() {
  let _ = id(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_alias_propagates_comparable_to_struct_field_use() {
    let input = r#"
type Table<T> = Map<T, int>

struct Box<T> {
  table: Table<T>,
}

fn main() {
  let b = Box { table: Map.new<string, int>() }
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_alias_propagates_comparable_to_function_signature() {
    let input = r#"
type Table<T> = Map<T, int>

fn id<T>(table: Table<T>) -> Table<T> {
  table
}

fn main() {
  let t = Map.new<string, int>()
  let _ = id(t)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_alias_propagates_comparable_to_enum_variant() {
    let input = r#"
type Table<T> = Map<T, int>

enum Holder<T> {
  Some(Table<T>),
}

fn main() {
  let h = Holder.Some(Map.new<string, int>())
  let _ = h
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_alias_propagates_comparable_to_interface_method() {
    let input = r#"
type Table<T> = Map<T, int>

interface Uses<T> {
  fn id(self, table: Table<T>) -> Table<T>
}

fn main() {
  let _ = 1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_key_use_in_let_initializer_constrains_generic() {
    let input = r#"
fn is_empty<T>() -> bool {
  let empty = Map.new<T, int>().is_empty()
  empty
}

fn main() {
  let _ = is_empty<int>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_key_use_in_tail_expression_constrains_generic() {
    let input = r#"
fn is_empty<T>() -> bool {
  Map.new<T, int>().is_empty()
}

fn main() {
  let _ = is_empty<int>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_key_use_in_if_condition_constrains_generic() {
    let input = r#"
fn score<T>() -> int {
  if Map.new<T, int>().is_empty() {
    1
  } else {
    0
  }
}

fn main() {
  let _ = score<int>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn explicit_named_bound_merges_with_inferred_comparable_on_function() {
    let input = r#"
interface Named {
  fn name(self) -> string
}

struct Person {
  value: string,
}

impl Person {
  fn name(self) -> string {
    self.value
  }
}

fn count<T: Named>(x: T) -> int {
  let mut m: Map<T, int> = Map.new()
  m[x] = 1
  m.length()
}

fn main() {
  let _ = count(Person { value: "Ada" })
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn explicit_named_bound_merges_with_inferred_comparable_on_struct() {
    let input = r#"
interface Named {
  fn name(self) -> string
}

struct Person {
  value: string,
}

impl Person {
  fn name(self) -> string {
    self.value
  }
}

struct Box<T: Named> {
  table: Map<T, int>,
}

fn main() {
  let p = Person { value: "Ada" }
  let _ = p.name()
  let b: Box<Person> = Box { table: Map.new() }
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn ordered_bound_plus_map_key_use_stays_cmp_ordered() {
    let input = r#"
fn count<T: Ordered>(x: T) -> int {
  let mut m: Map<T, int> = Map.new()
  m[x] = 1
  m.length()
}

fn main() {
  let _ = count(1)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn alias_chain_propagates_comparable_through_two_steps() {
    let input = r#"
type B<T> = Map<T, int>
type A<T> = B<T>

struct Box<T> {
  table: A<T>,
}

fn main() {
  let b = Box { table: Map.new<string, int>() }
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn imported_types_named_like_prelude_wrappers_are_not_confused() {
    let input = r#"
import "go:example.com/box"

fn use_ref(r: box.Ref<int>) -> int {
  r.inner
}

fn use_slice(s: box.Slice<int>) -> int {
  s.value
}

fn use_map(m: box.Map<string, int>) -> int {
  m.val
}

fn make_ref() -> box.Ref<int> {
  box.Ref { inner: 1 }
}

fn maybe_ref(r: box.Ref<int>) -> Option<box.Ref<int>> {
  Some(r)
}

struct Holder {
  r: box.Ref<int>,
  s: box.Slice<int>,
  m: box.Map<string, int>,
}
"#;
    let typedef = r#"
pub struct Ref<T> {
  pub inner: T,
}

pub struct Slice<T> {
  pub value: T,
}

pub struct Map<K, V> {
  pub key: K,
  pub val: V,
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/box", typedef)]);
}

#[test]
fn prelude_wrappers_still_lower_to_native_go_forms() {
    let input = r#"
fn use_ref(r: Ref<int>) -> Ref<int> {
  r
}

struct Holder {
  r: Ref<int>,
  s: Slice<int>,
  m: Map<string, int>,
}
"#;
    assert_emit_snapshot!(input);
}

const ANON_STRUCT_TYPEDEF: &str = r#"
#[go(anon_struct)]
pub struct AnonX {
  pub X: int,
}

pub fn PlainReturn() -> AnonX
pub fn TakesAnon(p: AnonX)
"#;

#[test]
fn go_anon_struct_read_renders_structurally() {
    let input = r#"
import anon "go:example.com/anon"

fn read() -> int {
  let r = anon.PlainReturn()
  r.X
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/anon", ANON_STRUCT_TYPEDEF)]);
}

#[test]
fn go_anon_struct_construction_renders_structurally() {
    let input = r#"
import anon "go:example.com/anon"

fn build() {
  anon.TakesAnon(anon.AnonX { X: 1 })
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/anon", ANON_STRUCT_TYPEDEF)]);
}

#[test]
fn go_anon_struct_annotation_renders_structurally() {
    let input = r#"
import anon "go:example.com/anon"

fn annotated() -> int {
  let x: anon.AnonX = anon.PlainReturn()
  x.X
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/anon", ANON_STRUCT_TYPEDEF)]);
}

#[test]
fn go_anon_struct_zero_value_renders_structurally() {
    let input = r#"
import anon "go:example.com/anon"

fn zero() {
  anon.TakesAnon(anon.AnonX { .. })
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/anon", ANON_STRUCT_TYPEDEF)]);
}

#[test]
fn conditional_impl_renamed_param_keeps_receiver_bound() {
    let input = r#"
struct Box<T> { value: T }

impl<U: Ordered> Box<U> {
  fn less(self, other: Box<U>) -> bool {
    self.value < other.value
  }
}

fn main() {
  let a = Box { value: 1 }
  let b = Box { value: 2 }
  let _ = a.less(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn clone_nested_slice_deep() {
    let input = r#"
fn main() {
  let a = [[1, 2], [3, 4]]
  let mut b = a.clone()
  b[0][0] = 9
  let _ = a
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn clone_map_slice_values_deep() {
    let input = r#"
fn main() {
  let m = Map.from([("k", [1, 2])])
  let mut m2 = m.clone()
  m2["k"] = [9]
  let _ = m
  let _ = m2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn clone_slice_of_tuples_deep() {
    let input = r#"
fn main() {
  let a = [(1, [2, 3])]
  let mut b = a.clone()
  b[0] = (4, [5])
  let _ = a
  let _ = b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn clone_user_method_dispatched() {
    let input = r#"
struct Doc { tags: Slice<string> }

impl Doc {
  fn clone(self) -> Doc {
    Doc { tags: self.tags.clone() }
  }
}

fn main() {
  let d1 = Doc { tags: ["x"] }
  let mut d2 = d1.clone()
  d2.tags[0] = "y"
  let _ = d1
  let _ = d2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn clone_enumerated_slice() {
    let input = r#"
fn main() {
  let s = [1, 2, 3]
  let e = s.enumerate()
  let mut e2 = e.clone()
  e2 = s.enumerate()
  let _ = e
  let _ = e2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn clone_container_of_user_clone_stays_shallow() {
    let input = r#"
struct Doc { tags: Slice<string> }

impl Doc {
  fn clone(self) -> Doc {
    Doc { tags: self.tags.clone() }
  }
}

fn main() {
  let docs = [Doc { tags: ["x"] }]
  let copy = docs.clone()
  let _ = docs
  let _ = copy
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_struct_param_shadows_module_type() {
    let input = r#"
struct T {
  v: int,
}

fn make_t() -> T {
  T { v: 7 }
}

struct Box<T> {
  item: T,
}

impl<T> Box<T> {
  fn probe(self) -> int {
    let mut opt = None
    opt = Some(make_t())
    match opt {
      Some(t) => t.v,
      None => 0,
    }
  }
}

fn test() -> int {
  let b = Box { item: 42 }
  b.probe()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn generic_enum_param_shadows_module_type() {
    let input = r#"
struct T {
  v: int,
}

enum Wrap<T> {
  Empty,
  Full(T),
}

fn test() -> int {
  let w = Wrap.Full(99)
  match w {
    Wrap.Empty => 0,
    Wrap.Full(n) => n,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_var_freshens_against_colliding_type_param() {
    let input = r#"
struct Box<b> {
  item: b,
}

impl<b> Box<b> {
  fn value(self) -> b {
    self.item
  }
}

fn test() -> int {
  let boxed = Box { item: 7 }
  boxed.value()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn synthesized_struct_methods_freshen_receiver_against_type_param() {
    let input = r#"
#[display]
#[equality]
struct Box<b: Comparable> {
  pub item: b,
}

fn test() -> bool {
  let x = Box { item: 7 }
  let y = Box { item: 7 }
  x == y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn synthesized_enum_methods_freshen_receiver_against_type_param() {
    let input = r#"
#[display]
#[json]
enum Box<b> {
  Empty,
  Full(b),
}

fn test() -> string {
  let x = Box.Full(7)
  x.to_string()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn synthesized_equals_freshens_param_against_type_param() {
    let input = r#"
#[equality]
struct Box<other: Comparable> {
  pub item: other,
}

fn test() -> bool {
  Box { item: 7 } == Box { item: 7 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn synthesized_json_freshens_param_against_type_param() {
    let input = r#"
#[json]
enum Box<data> {
  Empty,
  Full(data),
}

fn test() -> int {
  match Box.Full(7) {
    Box.Full(n) => n,
    Box.Empty => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn synthesized_json_freshens_local_against_receiver() {
    let input = r#"
#[json]
enum Vault {
  Full { a: int, b: int },
}

fn test() -> int {
  match Vault.Full { a: 1, b: 2 } {
    Vault.Full { a, b } => a + b,
  }
}
"#;
    assert_emit_snapshot!(input);
}
