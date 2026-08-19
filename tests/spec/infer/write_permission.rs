use crate::_harness::infer;

#[test]
fn direct_alias_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut b = a
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn block_tail_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut b = { a }
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn if_tail_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let c = [4]
  let mut b = if a.length() > 0 { a } else { c }
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn construction_demotes_write_refused() {
    infer(
        r#"struct Box2 { items: mut Slice<int> }
fn main() {
  let a = [1, 2, 3]
  let mut boxed = Box2 { items: a }
  boxed.items[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn tuple_component_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut pair = (a, 0)
  pair.0[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn slice_literal_element_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut b = [a]
  b[0][0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn projection_argument_refused() {
    infer(
        r#"import "go:slices"
struct Holder2 { items: mut Slice<int> }
fn main() {
  let s = Holder2 { items: [3, 1, 2] }
  slices.Sort(s.items)
}"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn subslice_binds_write_refused() {
    infer(
        r#"fn main() {
  let data = [1, 2, 3, 4]
  let mut rest = data[1..]
  rest = rest[1..]
  rest[0] = 9
  let _ = data
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn for_element_write_refused() {
    infer(
        r#"fn main() {
  let outer = [[1], [2]]
  for row in outer {
    row[0] = 9
  }
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

// Writes that used to slip through, now refused at the write or the store.

#[test]
fn spread_demotes_write_refused() {
    infer(
        r#"struct Box2 { items: mut Slice<int>, n: int }
fn main() {
  let orig = Box2 { items: [1], n: 1 }
  let mut b = Box2 { n: 5, ..orig }
  b.items[0] = 99
  let _ = orig
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn element_store_needs_writable() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut outer = [[9]]
  outer[0] = a
  let _ = a
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn map_store_needs_writable() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut m = Map.new<string, mut Slice<int>>()
  m["k"] = a
  let _ = a
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn block_local_indirection_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut b = { let x = a  x }
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn match_payload_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let opt = Some(a)
  let mut b = match opt { Some(x) => x, None => [0] }
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn tuple_variant_payload_refused_at_argument() {
    // Deviation from the matrix, recorded there: the tuple-variant
    // constructor checks its payload argument directly.
    infer(
        r#"enum Holder { Tags(mut Slice<string>), Empty }
fn main() {
  let a = ["x"]
  let h = Holder.Tags(a)
  let _ = h
  let _ = a
}"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn reassign_into_construction_write_refused() {
    infer(
        r#"struct Wrap2 { items: mut Slice<int> }
fn main() {
  let a = [1, 2, 3]
  let mut w = Wrap2 { items: [9] }
  w = Wrap2 { items: a }
  w.items[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn map_from_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut m = Map.from([("k", a)])
  m["k"][0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn struct_variant_payload_write_refused() {
    infer(
        r#"enum Holder { Tags { items: mut Slice<string> }, Empty }
fn main() {
  let a = ["x"]
  let h = Holder.Tags { items: a }
  match h {
    Holder.Tags { items } => { items[0] = "y" },
    Holder.Empty => {},
  }
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn field_store_needs_writable() {
    infer(
        r#"struct Wrap2 { items: mut Slice<int> }
fn main() {
  let a = [1, 2, 3]
  let mut w = Wrap2 { items: [9] }
  w.items = a
  let _ = a
}"#,
    )
    .assert_infer_code("needs_writable");
}

// Level 2 rows, closed by the type travelling through calls.

#[test]
fn identity_return_write_refused() {
    infer(
        r#"fn identity(x: Slice<int>) -> Slice<int> { x }
fn main() {
  let a = [1, 2, 3]
  let mut b = identity(a)
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn subslice_return_write_refused() {
    infer(
        r#"fn head(x: Slice<int>) -> Slice<int> { x[0..2] }
fn main() {
  let a = [1, 2, 3]
  let mut b = head(a)
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn laundered_argument_refused() {
    infer(
        r#"import "go:sort"
fn identity(x: Slice<int>) -> Slice<int> { x }
fn main() {
  let a = [3, 1, 2]
  sort.Ints(identity(a))
  let _ = a
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn receiver_projection_write_refused() {
    infer(
        r#"struct S2 { items: mut Slice<int> }
impl S2 {
  fn view(self) -> Slice<int> { self.items }
}
fn main() {
  let s = S2 { items: [1, 2] }
  let mut x = s.view()
  x[0] = 99
  let _ = s
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn closure_return_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let f = || a
  let mut b = f()
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn reassignment_needs_writable() {
    infer(
        r#"fn identity(x: Slice<int>) -> Slice<int> { x }
fn main() {
  let a = [1, 2, 3]
  let mut b = [1]
  b = identity(a)
  let _ = a
  let _ = b
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn generic_identity_write_refused() {
    infer(
        r#"fn identity<T>(x: T) -> T { x }
fn main() {
  let a = [1, 2, 3]
  let mut b = identity(a)
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn ref_param_store_refused_in_callee() {
    infer(
        r#"struct Wrap2 { items: mut Slice<int> }
fn stash(w: Ref<Wrap2>, v: Slice<int>) {
  w.items = v
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn shadow_parameter_write_refused() {
    infer(
        r#"fn poke(data: Slice<int>) {
  let mut data = data
  data[0] = 99
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn shadow_argument_needs_writable() {
    infer(
        r#"fn touch(items: mut Slice<int>) {
  items[0] = 99
}
fn main() {
  let xs = [1, 2, 3]
  let mut xs = xs
  touch(xs)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn identical_place_aliased_argument_refused() {
    infer(
        r#"fn touch(a: mut Slice<int>, b: Slice<int>) {
  a[0] = 99
  let _ = b
}
fn main() {
  let mut xs = [1, 2, 3]
  touch(xs, xs)
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

// Level 2b, the Go boundary, gated on the derived facts.

#[test]
fn trimspace_view_write_refused() {
    infer(
        r#"import "go:bytes"
fn main() {
  let data: Slice<byte> = [32, 104, 32]
  let mut t = bytes.TrimSpace(data)
  t[0] = 88
  let _ = data
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn clip_view_argument_refused() {
    infer(
        r#"import "go:sort"
import "go:slices"
fn main() {
  let a = [3, 1, 2]
  sort.Ints(slices.Clip(a))
  let _ = a
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn sort_immutable_refused() {
    infer(
        r#"import "go:slices"
fn main() {
  let a = [3, 1, 2]
  slices.Sort(a)
  let _ = a
}"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn compact_chain_accepted() {
    infer(
        r#"import "go:slices"
fn main() {
  let mut s = [1, 1, 2]
  s = slices.Compact(s)
  let _ = s
}"#,
    )
    .assert_no_errors();
}

#[test]
fn element_view_write_refused() {
    infer(
        r#"fn main() {
  let inner = [1, 2, 3]
  let a = [inner, inner]
  match a.get(0) {
    Some(g) => { g[0] = 99 },
    None => {},
  }
  let _ = inner
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

// Taking `&` of a read-only place must not produce a writable pointer.

#[test]
fn ref_of_immutable_binding_write_refused() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut r = &a
  r.*[0] = 99
  let _ = a
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

// Programs that must stay accepted: false positives the model removes.

#[test]
fn fresh_block_local_accepted() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut b = { let mut x = [9, 9]  x }
  b[0] = 0
  let _ = a
  let _ = b
}"#,
    )
    .assert_no_errors();
}

#[test]
fn deep_clone_accepted() {
    infer(
        r#"fn main() {
  let inner = [1, 2]
  let a = [inner, inner]
  let mut b = a.clone()
  b[0][0] = 99
  let _ = a
}"#,
    )
    .assert_no_errors();
}

#[test]
fn append_fresh_accepted() {
    infer(
        r#"fn main() {
  let a = [1, 2, 3]
  let mut b = a.append(4)
  b[0] = 9
  let _ = a
}"#,
    )
    .assert_no_errors();
}

#[test]
fn encoder_cursor_accepted() {
    infer(
        r#"fn encode(dst: mut Slice<byte>, src: Slice<byte>) -> int {
  let mut cursor = dst
  let mut written = 0
  for b in src {
    if cursor.is_empty() { break }
    cursor[0] = b
    cursor = cursor[1..]
    written += 1
  }
  written
}"#,
    )
    .assert_no_errors();
}

#[test]
fn nested_element_writes_accepted() {
    infer(
        r#"fn double_all(rows: mut Slice<mut Slice<int>>) {
  for mut row in rows {
    row[0] = row[0] * 2
  }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn struct_state_flagship_accepted() {
    infer(
        r#"struct Index2 {
  counts: mut Map<string, int>,
  order: mut Slice<string>,
}
impl Index2 {
  fn add(self: mut Ref<Index2>, word: string) {
    self.counts[word] = self.counts.get(word).unwrap_or(0) + 1
    self.order = self.order.append(word)
  }
  fn words(self: Ref<Index2>) -> Slice<string> { self.order }
}
fn main() {
  let mut index = Index2 { counts: Map.new<string, int>(), order: Slice.new<string>() }
  index.add("hello")
  let _ = index.words()
}"#,
    )
    .assert_no_errors();
}

// The four hardening fixes from the deep review.

#[test]
fn nested_ref_qualifier_checked_per_hop() {
    infer(
        r#"fn bump(r: mut Ref<mut Ref<int>>) {
  r.*.* = 9
}
fn main() {
  let y = 0
  let mut inner = &y
  let mut outer = &inner
  bump(outer)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn auto_deref_innermost_hop_governs() {
    infer(
        r#"struct S3 { x: int }
fn poke(r: mut Ref<Ref<S3>>) {
  r.x = 1
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn receiver_aliased_with_argument_refused() {
    infer(
        r#"fn main() {
  let mut xs = [1, 2, 3]
  let _ = xs.copy_from(xs)
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

#[test]
fn computed_indexes_are_not_assumed_aliased() {
    infer(
        r#"fn touch(a: mut Slice<int>, b: Slice<int>) {
  a[0] = 99
  let _ = b
}
fn main() {
  let mut rows = [[1], [2], [3]]
  let i = 0
  let j = 1
  touch(rows[i + 1], rows[j + 1])
}"#,
    )
    .assert_no_errors();
}

#[test]
fn identical_computed_index_refused() {
    infer(
        r#"fn touch(a: mut Slice<int>, b: Slice<int>) {
  a[0] = 99
  let _ = b
}
fn main() {
  let mut rows = [[1], [2], [3]]
  let i = 0
  touch(rows[i + 1], rows[i + 1])
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

#[test]
fn nested_call_does_not_steal_the_receiver() {
    infer(
        r#"fn helper() -> Slice<int> { [7] }
fn main() {
  let mut xs = [1, 2, 3]
  let _ = xs.copy_from({ let h = helper()  h })
  let _ = xs.copy_from(xs)
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

#[test]
fn cast_cannot_restore_permission() {
    infer(
        r#"type UserId = int
fn main() {
  let a = [1, 2, 3]
  let ids = a as mut Slice<UserId>
  let _ = ids
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn cast_cannot_restore_permission_through_alias() {
    infer(
        r#"type Ints = Slice<int>
fn main() {
  let a = [1, 2, 3]
  let ids = a as mut Ints
  let _ = ids
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn cast_may_demote_permission() {
    infer(
        r#"type UserId = int
fn main() {
  let mut a = [1, 2, 3]
  let ids = a as Slice<UserId>
  let _ = ids
}"#,
    )
    .assert_no_errors();
}

#[test]
fn identical_selector_index_refused() {
    infer(
        r#"struct State { i: int }
fn touch(a: mut Slice<int>, b: Slice<int>) {
  a[0] = 99
  let _ = b
}
fn main() {
  let mut rows = [[1], [2]]
  let state = State { i: 0 }
  touch(rows[state.i], rows[state.i])
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

#[test]
fn identical_range_index_refused() {
    infer(
        r#"fn touch(a: mut Slice<int>, b: Slice<int>) {
  a[0] = 99
  let _ = b
}
fn main() {
  let mut rows = [1, 2, 3, 4]
  let i = 0
  let j = 2
  touch(rows[i..j], rows[i..j])
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

#[test]
fn declared_sharing_accepted() {
    infer(
        r#"fn main() {
  let mut a = [1, 2, 3]
  let mut b = a
  b[0] = 99
  let _ = a
}"#,
    )
    .assert_no_errors();
}

#[test]
fn read_only_value_into_mut_unknown_refused() {
    infer(
        r#"fn sink(x: mut Unknown) {
  let _ = x
}
fn main() {
  let a = [1, 2, 3]
  let view: Slice<int> = a
  sink(view)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn writable_value_into_mut_unknown_accepted() {
    infer(
        r#"fn sink(x: mut Unknown) {
  let _ = x
}
fn main() {
  let mut a = [1, 2, 3]
  sink(a)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_value_into_mut_unknown_refused() {
    infer(
        r#"fn sink(x: mut Unknown) {
  let _ = x
}
fn pass(u: Unknown) {
  sink(u)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn mut_unknown_value_into_mut_unknown_accepted() {
    infer(
        r#"fn sink(x: mut Unknown) {
  let _ = x
}
fn pass(u: mut Unknown) {
  sink(u)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn assert_type_to_writable_refused() {
    infer(
        r#"fn narrow(u: Unknown) {
  let _ = assert_type<mut Slice<int>>(u)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn assert_type_to_nested_writable_refused() {
    infer(
        r#"struct Box<T> {
  value: T
}
fn narrow(u: Unknown) {
  let _ = assert_type<Box<mut Slice<int>>>(u)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn assert_type_to_writable_alias_refused() {
    infer(
        r#"type Rows = mut Slice<int>
fn narrow(u: Unknown) {
  let _ = assert_type<Rows>(u)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn assert_type_to_read_only_accepted() {
    infer(
        r#"fn narrow(u: Unknown) {
  let _ = assert_type<Slice<int>>(u)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn option_payload_survives_plain_let() {
    infer(
        r#"fn make() -> Option<mut Slice<int>> {
  Some([1, 2, 3])
}
fn main() {
  let maybe = make()
  match maybe {
    Some(xs) => xs[0] = 9,
    None => {}
  }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn tuple_component_survives_plain_let() {
    infer(
        r#"fn make() -> (mut Slice<int>, int) {
  ([1, 2, 3], 0)
}
fn main() {
  let pair = make()
  let (xs, _) = pair
  xs[0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn channel_payload_survives_plain_let() {
    infer(
        r#"fn main() {
  let ch = Channel.new<mut Slice<int>>()
  let (tx, rx) = ch.split()
  let mut xs = [1, 2]
  let _ = tx.send(xs)
  match rx.receive() {
    Some(got) => got[0] = 9,
    None => {}
  }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn promoted_field_gated_by_owner() {
    infer(
        r#"struct Rows {
  items: mut Slice<int>
}
struct Report {
  embed Rows
}
fn poke(r: Report) {
  r.items[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn promoted_method_needs_writable_receiver() {
    infer(
        r#"struct Rows {
  items: mut Slice<int>
}
impl Rows {
  fn zero(self: mut Ref<Rows>) {
    self.items[0] = 0
  }
}
struct Report {
  embed Rows
}
fn poke(r: Report) {
  r.zero()
}"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn promoted_method_through_writable_chain_accepted() {
    infer(
        r#"struct Rows {
  items: mut Slice<int>
}
impl Rows {
  fn zero(self: mut Ref<Rows>) {
    self.items[0] = 0
  }
}
struct Report {
  embed Rows
}
fn poke(r: mut Ref<Report>) {
  r.zero()
}"#,
    )
    .assert_no_errors();
}

#[test]
fn immutable_loop_binding_names_the_index_write() {
    let result = infer(
        r#"struct Todo { done: bool }
fn finish(todos: mut Slice<Todo>) {
  for todo in todos {
    todo.done = true
  }
}"#,
    );
    result.assert_infer_code_once("immutable");
}

#[test]
fn loop_copy_write_warns() {
    infer(
        r#"struct Todo { done: bool }
fn finish(todos: mut Slice<Todo>) {
  for mut todo in todos {
    todo.done = true
  }
}"#,
    )
    .assert_infer_code_once("loop_copy_write");
}

#[test]
fn loop_element_write_through_does_not_warn() {
    infer(
        r#"fn double_all(rows: mut Slice<mut Slice<int>>) {
  for mut row in rows {
    row[0] = row[0] * 2
  }
}"#,
    )
    .assert_infer_code_count("loop_copy_write", 0);
}

#[test]
fn shadowed_loop_binding_write_does_not_warn() {
    infer(
        r#"struct Todo { done: bool }
fn scan(todos: Slice<Todo>) {
  for todo in todos {
    let mut todo = Todo { done: todo.done }
    todo.done = true
    let _ = todo
  }
}"#,
    )
    .assert_infer_code_count("loop_copy_write", 0);
}

#[test]
fn mut_on_scalar_refused() {
    infer(
        r#"fn double(n: mut int) -> int {
  n * 2
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_scalar_field_refused() {
    infer(
        r#"struct Counter {
  n: mut int
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_scalar_let_refused() {
    infer(
        r#"fn main() {
  let x: mut int = 1
  let _ = x
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_impl_target_refused() {
    infer(
        r#"struct Batch {
  items: mut Slice<int>,
}

impl mut Batch {
  fn first(self) -> int {
    self.items[0]
  }
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn read_only_reference_does_not_satisfy_writing_interface() {
    infer(
        r#"interface Sink {
  fn push(value: int)
}

struct Bag {
  items: mut Slice<int>,
}

impl Bag {
  fn push(self: mut Ref<Bag>, value: int) {
    self.items[0] = value
  }
}

fn feed(s: Sink) {
  s.push(1)
}

fn main() {
  let bag = Bag { items: [0] }
  feed(&bag)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn writable_reference_satisfies_writing_interface() {
    infer(
        r#"interface Sink {
  fn push(value: int)
}

struct Bag {
  items: mut Slice<int>,
}

impl Bag {
  fn push(self: mut Ref<Bag>, value: int) {
    self.items[0] = value
  }
}

fn feed(s: Sink) {
  s.push(1)
}

fn main() {
  let mut bag = Bag { items: [0] }
  feed(&bag)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_type_parameter_refused() {
    infer(
        r#"fn hold<T>(x: mut T) {
  let _ = x
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_struct_without_writable_content_refused() {
    infer(
        r#"struct Point {
  x: int
}
fn shift(p: mut Point) {
  let _ = p
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_function_alias_refused() {
    infer(
        r#"type Handler = fn(int) -> int
fn install(h: mut Handler) {
  let _ = h
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_array_refused() {
    infer(
        r#"fn hold(a: mut Array<int, 3>) {
  let _ = a
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_tuple_alias_refused() {
    infer(
        r#"type Pair = (mut Slice<int>, int)
fn hold(p: mut Pair) {
  let _ = p
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_function_alias_with_writable_return_refused() {
    infer(
        r#"type Factory = fn() -> mut Slice<int>
fn install(g: mut Factory) {
  let _ = g
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_writable_alias_accepted() {
    infer(
        r#"type Rows = mut Slice<int>
fn set_first(r: mut Rows) {
  r[0] = 1
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_generic_tuple_alias_refused() {
    infer(
        r#"type Pair<T> = (T, int)
fn hold<T>(p: mut Pair<T>) {
  let _ = p
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_identity_alias_refused() {
    infer(
        r#"type Identity<T> = T
fn hold(x: mut Identity<Slice<int>>) {
  let _ = x
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_generic_slice_alias_accepted() {
    infer(
        r#"type Grid<T> = Slice<T>
fn fill<T>(g: mut Grid<T>, v: T) {
  g[0] = v
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_newtype_of_slice_accepted() {
    infer(
        r#"type Row = Slice<int>
fn set_first(items: mut Row) {
  items[0] = 1
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_struct_with_writable_field_accepted() {
    infer(
        r#"struct Holder {
  items: mut Slice<int>
}
fn hold(h: mut Holder) {
  let _ = h
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_enum_with_writable_payload_accepted() {
    infer(
        r#"enum Load {
  Data(mut Slice<int>),
  Empty
}
fn hold(load: mut Load) {
  let _ = load
}"#,
    )
    .assert_no_errors();
}

#[test]
fn writable_reference_to_loop_copy_warns() {
    infer(
        r#"struct Todo { done: bool }
fn finish(todo: mut Ref<Todo>) { todo.*.done = true }
fn main() {
  let todos = [Todo { done: false }]
  for mut todo in todos { finish(&todo) }
}"#,
    )
    .assert_infer_code_once("loop_copy_write");
}

#[test]
fn function_field_return_gated_by_owner() {
    infer(
        r#"struct Factory { get: fn() -> mut Slice<int> }
fn change(factory: Factory) {
  (factory.get)()[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn function_field_return_writable_through_mut_owner_accepted() {
    infer(
        r#"struct Factory { get: fn() -> mut Slice<int> }
fn change(factory: mut Ref<Factory>) {
  (factory.get)()[0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn assert_type_to_type_parameter_refused() {
    infer(
        r#"fn narrow<T>(value: Unknown) -> Option<T> {
  assert_type<T>(value)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn assert_type_to_parameter_carrier_refused() {
    infer(
        r#"struct Box<T> { value: T }
fn narrow<T>(value: Unknown) -> Option<Box<T>> {
  assert_type<Box<T>>(value)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn destructure_let_of_writable_struct_accepted() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
fn write(holder: mut Holder) {
  let Holder { items } = holder
  items[0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_alias_of_flagless_carrier_refused() {
    infer(
        r#"type Maybe = Option<mut Slice<int>>
fn hold(value: mut Maybe) {
  let _ = value
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn nested_qualifier_invariance_refused() {
    infer(
        r#"fn replace<T>(value: T, outer: mut Slice<T>) {
  outer[0] = value
}
fn main() {
  let view = [2]
  let mut outer = [[1]]
  replace(view, outer)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn mut_on_flagless_generic_carrier_refused() {
    infer(
        r#"struct Box<T> {
  value: T
}
fn hold(b: mut Box<mut Slice<int>>) {
  let _ = b
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_option_refused() {
    infer(
        r#"fn hold(value: mut Option<mut Slice<int>>) {
  let _ = value
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_result_refused() {
    infer(
        r#"fn hold(value: mut Result<mut Slice<int>, string>) {
  let _ = value
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn mut_on_generic_struct_with_scalar_argument_refused() {
    infer(
        r#"struct Box<T> {
  value: T
}
fn hold(b: mut Box<int>) {
  let _ = b
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn ref_to_demoted_construction_field_write_refused() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
fn poke(a: Slice<int>) {
  let mut h = Holder { items: a }
  let mut p = &h
  p.items[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn receiver_method_on_demoted_construction_refused() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
impl Holder {
  fn zap(self: mut Ref<Holder>) { self.items[0] = 9 }
}
fn poke(a: Slice<int>) {
  let mut h = Holder { items: a }
  h.zap()
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn ref_of_demoted_fresh_construction_write_refused() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
fn poke(a: Slice<int>) {
  let mut p = &Holder { items: a }
  p.items[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn writable_holder_through_ref_both_spellings_accepted() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
impl Holder {
  fn zap(self: mut Ref<Holder>) { self.items[0] = 9 }
}
fn main() {
  let mut h = Holder { items: [1, 2] }
  h.zap()
  let mut p = &h
  p.items[0] = 8
  p.*.items[1] = 7
}"#,
    )
    .assert_no_errors();
}

#[test]
fn deep_permission_mismatch_demotes_construction() {
    infer(
        r#"struct Grid { rows: mut Slice<mut Slice<int>> }
fn poke(a: Slice<int>) {
  let mut rows = [a]
  let mut g = Grid { rows: rows }
  g.rows[0][0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn nested_declared_permission_construction_accepted() {
    infer(
        r#"struct Holder { maybe: Option<mut Slice<int>> }
fn make() -> mut Holder {
  Holder { maybe: Some([1, 2]) }
}
fn make_empty() -> mut Holder {
  Holder { maybe: None }
}
fn main() {
  let _ = make()
  let _ = make_empty()
}"#,
    )
    .assert_no_errors();
}

#[test]
fn read_only_payload_into_writable_component_refused() {
    infer(
        r#"struct Holder { maybe: Option<mut Slice<int>> }
fn wrap(view: Slice<int>) -> mut Holder {
  Holder { maybe: Some(view) }
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn tuple_field_permission_construction_accepted() {
    infer(
        r#"struct Pair { parts: (mut Slice<int>, int) }
fn make() -> mut Pair {
  Pair { parts: ([1, 2], 0) }
}
fn main() {
  let _ = make()
}"#,
    )
    .assert_no_errors();
}

#[test]
fn method_value_then_unrelated_call_accepted() {
    infer(
        r#"struct Counter { n: int }
impl Counter {
  fn set(self: mut Ref<Counter>, v: int) { self.n = v }
}
fn observe(c: Counter) -> int { c.n }
fn main() {
  let mut c = Counter { n: 0 }
  let f = c.set
  let seen = observe(c)
  f(seen)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn fn_element_of_read_only_slice_write_refused() {
    infer(
        r#"fn invoke(fs: Slice<fn() -> mut Slice<int>>) {
  fs[0]()[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn scalar_field_write_through_ref_of_demoted_construction_accepted() {
    infer(
        r#"struct Holder { items: mut Slice<int>, n: int }
fn update(a: Slice<int>) -> int {
  let mut h = Holder { items: a, n: 0 }
  let mut p = &h
  p.n = 1
  h.n
}"#,
    )
    .assert_no_errors();
}

#[test]
fn self_referential_writable_ref_field_accepted() {
    infer(
        r#"struct Node { v: mut Slice<int>, next: Option<mut Ref<Node>> }
fn main() {
  let mut tail = Node { v: [1], next: None }
  let mut head = Node { v: [2], next: None }
  head.next = Some(&tail)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn alias_field_projection_demoted_refused() {
    infer(
        r#"type Items = mut Slice<int>
struct Holder { items: Items }
fn poke(h: Holder) {
  h.items[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn alias_binding_demotion_refused() {
    infer(
        r#"type Items = mut Slice<int>
fn poke(v: Items) {
  let w = v
  w[0] = 9
}"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn alias_parameter_write_accepted() {
    infer(
        r#"type Items = mut Slice<int>
fn poke(v: Items) {
  v[0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn newtype_of_writable_alias_demotes_refused() {
    infer(
        r#"type Items = mut Slice<int>
struct Wrapper(Items)
fn mutate(xs: mut Slice<int>) {
  xs[0] = 9
}
fn leak(w: Wrapper) {
  mutate(w)
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn newtype_of_writable_alias_cast_refused() {
    infer(
        r#"type Items = mut Slice<int>
struct Wrapper(Items)
fn leak(w: Wrapper) {
  let _ = w as mut Slice<int>
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn newtype_of_writable_alias_writable_owner_accepted() {
    infer(
        r#"type Items = mut Slice<int>
struct Wrapper(Items)
fn mutate(xs: mut Slice<int>) {
  xs[0] = 9
}
fn grant(w: mut Wrapper) {
  mutate(w)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn tuple_alias_component_survives_plain_let() {
    infer(
        r#"type Pair = (mut Slice<int>, int)
fn touch(p: Pair) {
  let q = p
  q.0[0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn aliased_writable_arguments_behind_alias_refused() {
    infer(
        r#"type Items = mut Slice<int>
fn touch(a: Items, b: Items) {
  a[0] = 9
}
fn main() {
  let mut xs = [1, 2, 3]
  touch(xs, xs)
}"#,
    )
    .assert_infer_code("aliased_writable_argument");
}

#[test]
fn lambda_initializer_grants_construction() {
    infer(
        r#"struct S { get: fn() -> mut Slice<int> }
fn need(s: mut S) {
  let _ = s
}
fn main() {
  let mut s = S { get: || [1, 2] }
  need(s)
}"#,
    )
    .assert_no_errors();
}

#[test]
fn lambda_initializer_cannot_fake_permission() {
    infer(
        r#"struct S { get: fn() -> mut Slice<int> }
fn hold(cached: Slice<int>) {
  let _ = S { get: || cached }
}"#,
    )
    .assert_infer_code("needs_writable");
}

#[test]
fn generic_call_initializer_construction_accepted() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
fn identity<T>(x: T) -> T { x }
fn make() -> mut Holder {
  Holder { items: identity([1, 2]) }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn unit_variant_of_gated_enum_writable_accepted() {
    infer(
        r#"enum Load {
  Data(mut Slice<int>),
  Empty,
}
fn empty() -> mut Load {
  Load.Empty
}"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_on_generic_carrier_with_writable_argument_refused() {
    infer(
        r#"struct Box<T> { value: Option<T> }
fn hold(b: mut Box<mut Slice<int>>) {
  let _ = b
}"#,
    )
    .assert_infer_code("mut_without_effect");
}

#[test]
fn forward_referenced_enum_payload_constructor_accepted() {
    infer(
        r#"enum Wrap { Holds(mut Ref<Node>) }
struct Node { v: mut Slice<int> }
fn main() {
  let mut node = Node { v: [1] }
  let w = Wrap.Holds(&node)
  let _ = w
}"#,
    )
    .assert_no_errors();
}

#[test]
fn generic_method_initializer_construction_accepted() {
    infer(
        r#"struct Holder { items: mut Slice<int> }
struct Factory { n: int }
impl Factory {
  fn make<T>(self: Ref<Factory>, x: T) -> T { x }
}
fn build(f: Factory) -> mut Holder {
  Holder { items: f.make([1, 2]) }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn method_on_literal_receiver_initializer_accepted() {
    infer(
        r#"struct Carrier { items: mut Slice<int> }
struct Id { n: int }
impl Id {
  fn call<T>(self: Ref<Id>, x: T) -> T { x }
}
fn build() -> mut Carrier {
  Carrier { items: Id { n: 0 }.call([1, 2]) }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn tuple_struct_read_only_input_demotes() {
    infer(
        r#"struct Holder(mut Slice<int>, int)
fn wrap(view: Slice<int>) -> Holder {
  Holder(view, 0)
}
fn poke(view: Slice<int>) {
  let mut h = wrap(view)
  h.0[0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn tuple_struct_writable_input_grants() {
    infer(
        r#"struct Holder(mut Slice<int>, int)
fn make() -> mut Holder {
  Holder([1, 2], 0)
}
fn main() {
  let mut h = make()
  h.0[0] = 5
}"#,
    )
    .assert_no_errors();
}

#[test]
fn fresh_prelude_producers_are_writable() {
    infer(
        r#"fn main() {
  let mut parts = "a,b".split(",")
  parts[0] = "z"
  let mut bytes = "abc".bytes()
  bytes[0] = 65
  let mut runes = "abc".runes()
  runes[0] = 'z'
  let fixed: Array<int, 3> = [1, 2, 3]
  let mut copied = fixed.to_slice()
  copied[0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn for_mut_element_write_counts_as_collection_use() {
    infer(
        r#"fn main() {
  let mut rows = [[1, 2], [3, 4]]
  for mut r in rows {
    r[0] = 9
  }
}"#,
    )
    .assert_no_errors();
}

#[test]
fn nested_literal_fills_writable_component() {
    infer(
        r#"struct Grid { rows: mut Slice<mut Slice<int>> }
fn main() {
  let mut g = Grid { rows: [[1]] }
  g.rows[0][0] = 9
}"#,
    )
    .assert_no_errors();
}

#[test]
fn nested_literal_holding_read_only_demotes_construction() {
    infer(
        r#"struct Grid { rows: mut Slice<mut Slice<int>> }
fn main() {
  let view = [1]
  let mut g = Grid { rows: [view] }
  g.rows[0][0] = 9
}"#,
    )
    .assert_infer_code("write_through_read_only");
}

#[test]
fn repeated_writes_to_one_place_report_once() {
    infer(
        r#"struct Doc { tags: Slice<string> }
fn main() {
  let mut d = Doc { tags: ["a", "b"] }
  d.tags[0] = "x"
  d.tags[1] = "y"
  d.tags[0] = "z"
}"#,
    )
    .assert_infer_code_once("write_through_read_only");
}
