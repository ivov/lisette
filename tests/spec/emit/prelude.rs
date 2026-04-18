use crate::assert_emit_snapshot;

#[test]
fn string_length() {
    let input = r#"
fn test(s: string) -> int {
  s.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_is_empty() {
    let input = r#"
fn test(s: string) -> bool {
  s.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_contains() {
    let input = r#"
fn test(s: string, sub: string) -> bool {
  s.contains(sub)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_new() {
    let input = r#"
fn test() -> Slice<int> {
  Slice.new<int>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_new_with_void_function_element() {
    let input = r#"
fn test() -> Slice<fn(int)> {
  Slice.new<fn(int)>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_length() {
    let input = r#"
fn test(s: Slice<int>) -> int {
  s.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_is_empty() {
    let input = r#"
fn test(s: Slice<int>) -> bool {
  s.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_capacity() {
    let input = r#"
fn test(s: Slice<int>) -> int {
  s.capacity()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_get() {
    let input = r#"
fn test(s: Slice<int>, i: int) -> Option<int> {
  s.get(i)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_get() {
    let input = r#"
fn test(m: Map<string, int>, key: string) -> Option<int> {
  m.get(key)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_append() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<int> {
  s.append(1, 2, 3)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_append_no_args() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<int> {
  s.append()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_append_reassign() {
    let input = r#"
fn test(items: Slice<int>) {
  let mut s = items
  s = s.append(1, 2, 3)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_extend() {
    let input = r#"
fn test(a: Slice<int>, b: Slice<int>) -> Slice<int> {
  a.extend(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_extend_reassign() {
    let input = r#"
fn test(items: Slice<int>, b: Slice<int>) {
  let mut a = items
  a = a.extend(b)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_append_statement() {
    let input = r#"
fn test(items: Slice<int>) -> Slice<int> {
  let mut s = items
  s = s.append(4)
  s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_extend_statement() {
    let input = r#"
fn test(items: Slice<int>, extra: Slice<int>) -> Slice<int> {
  let mut s = items
  s = s.extend(extra)
  s
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_tail_append_no_writeback() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<int> {
  let x = { s.append(2) }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_tail_append_unused_binding() {
    let input = r#"
fn test(s: Slice<int>) {
  let _x = { s.append(2) }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_copy_from() {
    let input = r#"
fn test(dst: Slice<int>, src: Slice<int>) -> int {
  dst.copy_from(src)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_filter() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<int> {
  s.filter(|x| x > 0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_map() {
    let input = r#"
fn test(s: Slice<int>, f: fn(int) -> string) -> Slice<string> {
  s.map(f)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_contains() {
    let input = r#"
fn test(s: Slice<int>, v: int) -> bool {
  s.contains(v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_fold() {
    let input = r#"
fn test(s: Slice<int>) -> int {
  s.fold(0, |acc, x| acc + x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_find() {
    let input = r#"
fn test(s: Slice<int>) -> Option<int> {
  s.find(|x| x > 0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_clone() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<int> {
  s.clone()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enumerated_slice_filter() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<(int, int)> {
  s.enumerate().filter(|(i, _)| i % 2 == 0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enumerated_slice_map() {
    let input = r#"
fn test(s: Slice<int>) -> Slice<int> {
  s.enumerate().map(|(i, v)| i * v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enumerated_slice_fold() {
    let input = r#"
fn test(s: Slice<int>) -> int {
  s.enumerate().fold(0, |acc, (i, v)| acc + i * v)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enumerated_slice_find() {
    let input = r#"
fn test(s: Slice<int>) -> Option<(int, int)> {
  s.enumerate().find(|(_, v)| v > 10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_string_join() {
    let input = r#"
fn test(items: Slice<string>) -> string {
  items.join(", ")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_string_map_filter_join() {
    let input = r#"
fn test(items: Slice<string>) -> string {
  items
    .map(|s| s + "!")
    .filter(|s| s.length() > 2)
    .join(", ")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_new() {
    let input = r#"
fn test() -> Map<string, int> {
  Map.new<string, int>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_length() {
    let input = r#"
fn test(m: Map<string, int>) -> int {
  m.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_is_empty() {
    let input = r#"
fn test(m: Map<string, int>) -> bool {
  m.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_delete() {
    let input = r#"
fn test(mut m: Map<string, int>, key: string) {
  m.delete(key)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_from_pairs() {
    let input = r#"
fn test() -> Map<string, int> {
  Map.from([("alice", 95), ("bob", 82)])
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_with_void_function_value() {
    let input = r#"
fn test() -> Map<string, fn()> {
  Map.new()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_clone() {
    let input = r#"
fn test(m: Map<string, int>) -> Map<string, int> {
  m.clone()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_new() {
    let input = r#"
fn test() -> Channel<int> {
  Channel.new<int>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_new_unit_type() {
    let input = r#"
fn test() -> Channel<()> {
  Channel.new<()>()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_length() {
    let input = r#"
fn test(ch: Channel<int>) -> int {
  ch.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_is_empty() {
    let input = r#"
fn test(ch: Channel<int>) -> bool {
  ch.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_capacity() {
    let input = r#"
fn test(ch: Channel<int>) -> int {
  ch.capacity()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_close() {
    let input = r#"
fn test(ch: Channel<int>) {
  ch.close()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn sender_length() {
    let input = r#"
fn test(s: Sender<int>) -> int {
  s.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn sender_is_empty() {
    let input = r#"
fn test(s: Sender<int>) -> bool {
  s.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn sender_capacity() {
    let input = r#"
fn test(s: Sender<int>) -> int {
  s.capacity()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn sender_close() {
    let input = r#"
fn test(s: Sender<int>) {
  s.close()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_length() {
    let input = r#"
fn test(r: Receiver<int>) -> int {
  r.length()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_capacity() {
    let input = r#"
fn test(r: Receiver<int>) -> int {
  r.capacity()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_is_empty() {
    let input = r#"
fn test(r: Receiver<int>) -> bool {
  r.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_complex() {
    let input = r#"
fn test() -> complex128 {
  complex(1.0, 2.0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_real() {
    let input = r#"
fn test(c: complex128) -> float64 {
  real(c)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_imaginary() {
    let input = r#"
fn test(c: complex128) -> float64 {
  imaginary(c)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_panic() {
    let input = r#"
fn test() {
  panic("something went wrong")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_panic_in_branch() {
    let input = r#"
fn test(x: int) -> int {
  if x < 0 {
    panic("negative value")
  } else {
    x
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn builtin_panic_with_error() {
    let input = r#"
fn test(err: error) {
  panic(err)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn string_is_empty_negated() {
    let input = r#"
fn test(s: string) -> bool {
  !s.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn slice_is_empty_negated() {
    let input = r#"
fn test(s: Slice<int>) -> bool {
  !s.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn map_is_empty_negated() {
    let input = r#"
fn test(m: Map<string, int>) -> bool {
  !m.is_empty()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn prelude_ufcs_static_call_option_map() {
    let input = r#"
fn main() {
  let opt = Some(1)
  let mapped = Option.map(opt, |x| x + 1)
  let _ = mapped
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn prelude_method_value_type_instantiation() {
    let input = r#"
fn main() {
  let f = Option.map
  let x = f(Some(1), |v| v + 1)
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}
