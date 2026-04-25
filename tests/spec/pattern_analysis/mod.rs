use crate::spec::infer::infer;

#[test]
fn test_exhaustive_enum_all_variants() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Option.Some(x) => x,
    Option.None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_non_exhaustive_enum() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Option.Some(x) => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_non_exhaustive_bool() {
    let input = r#"
match true {
  true => 1,
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_nested_enum_exhaustiveness() {
    let input = r#"
fn test(opt: Option<Option<int>>) -> int {
  match opt {
    Option.Some(Option.Some(x)) => x,
    Option.None => 0,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_exhaustive_with_wildcard() {
    let input = r#"
enum Color {
  Red,
  Green,
  Blue,
}

fn test(c: Color) -> int {
  match c {
    Color.Red => 1,
    _ => 2,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_wildcard_in_nested_pattern() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Option.Some(_) => 1,
    Option.None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_single_variant_enum() {
    let input = r#"
enum Single {
  OnlyOne,
}

fn test(s: Single) -> int {
  match s {
    Single.OnlyOne => 1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_non_exhaustive_string() {
    let input = r#"
match "hello" {
  "hello" => 1,
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_non_exhaustive_char() {
    let input = r#"
match 'a' {
  'a' => 1,
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_match_in_closure() {
    let input = r#"
fn test() {
  let f = |opt: Option<int>| {
    match opt {
      Option.Some(x) => x,
      Option.None => 0,
    }
  };
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_loop() {
    let input = r#"
fn test() {
  loop {
    let opt: Option<int> = Option.Some(1);
    match opt {
      Option.Some(x) => x,
      Option.None => 0,
    }
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_while() {
    let input = r#"
fn test() {
  while true {
    let opt: Option<int> = Option.Some(1);
    let _ = match opt {
      Option.Some(x) => x,
      Option.None => 0,
    };
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_for() {
    let input = r#"
fn test() {
  let xs = [1, 2, 3];
  for x in xs {
    let opt: Option<int> = Option.Some(x);
    let _ = match opt {
      Option.Some(y) => y,
      Option.None => 0,
    };
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_return() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  return match opt {
    Option.Some(x) => x,
    Option.None => 0,
  };
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_tuple() {
    let input = r#"
fn test(opt: Option<int>) {
  let x = match opt {
    Option.Some(x) => x,
    Option.None => 0,
  };
  let result = (x, 42);
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_binary_expression() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let x = match opt {
    Option.Some(x) => x,
    Option.None => 0,
  };
  x + 10
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_match_in_unary_expression() {
    let input = r#"
fn test(opt: Option<bool>) -> bool {
  let val = match opt {
    Option.Some(x) => x,
    Option.None => false,
  }
  !val
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_redundant_pattern() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Option.Some(x) => x,
    Option.None => 0,
    Option.None => 1,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_wildcard() {
    let input = r#"
match true {
  true => 1,
  false => 0,
  _ => 2,
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_after_wildcard() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    _ => 0,
    Option.Some(x) => x,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_exhaustive_slice_empty_and_rest() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [first, ..] => first,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_exhaustive_slice_with_wildcard() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    _ => 1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_non_exhaustive_slice_missing_empty() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [first, ..] => first,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_non_exhaustive_slice_fixed_length_only() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [a, b, c] => a + b + c,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_redundant_slice_pattern() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [first, ..] => first,
    [a, b] => a + b,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_slice_after_wildcard() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    _ => 0,
    [] => 1,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_exhaustive_slice_wildcard_only() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_exhaustive_slice_multiple_fixed_with_rest() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [a] => a,
    [a, b, ..] => a + b,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_non_exhaustive_slice_gaps_in_fixed() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [a, b] => a + b,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_redundant_slice_duplicate_empty() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [] => 1,
    _ => 2,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_slice_fixed_after_same_length() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [a] => a,
    [b] => b,
    _ => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_slice_fixed_after_rest() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [a, ..] => a,
    [x, y, z] => x + y + z,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_exhaustive_slice_rest_only() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [..] => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_redundant_slice_after_rest_only() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [..] => 0,
    [] => 1,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_exhaustive_slice_nested_enum() {
    let input = r#"
fn test(items: Slice<Option<int>>) -> int {
  match items {
    [] => 0,
    [Some(x), ..] => x,
    [None, ..] => -1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_non_exhaustive_slice_nested_enum_missing_variant() {
    let input = r#"
fn test(items: Slice<Option<int>>) -> int {
  match items {
    [] => 0,
    [Some(x), ..] => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_exhaustive_slice_nested_enum_with_wildcard_element() {
    let input = r#"
fn test(items: Slice<Option<int>>) -> int {
  match items {
    [] => 0,
    [_, ..] => 1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_redundant_slice_nested_enum_covered_by_wildcard() {
    let input = r#"
fn test(items: Slice<Option<int>>) -> int {
  match items {
    [] => 0,
    [_, ..] => 1,
    [Some(x), ..] => x,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_exhaustive_nested_slice() {
    let input = r#"
fn test(items: Slice<Slice<int>>) -> int {
  match items {
    [] => 0,
    [[], ..] => 1,
    [[x, ..], ..] => x,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_non_exhaustive_nested_slice_missing_inner_empty() {
    let input = r#"
fn test(items: Slice<Slice<int>>) -> int {
  match items {
    [] => 0,
    [[x, ..], ..] => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_non_exhaustive_nested_slice_missing_inner_nonempty() {
    let input = r#"
fn test(items: Slice<Slice<int>>) -> int {
  match items {
    [] => 0,
    [[], ..] => 1,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_enum_variant_with_bindings_not_wildcard() {
    let input = r#"
enum Pair<A, B> {
  Both(A, B),
  Neither,
}

fn test() -> int {
  let p = Pair.Both(3, 4);
  match p {
    Pair.Both(a, b) => a + b,
    Pair.Neither => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_mixed_qualified_unqualified_exhaustive() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) => x,
    Option.None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_mixed_qualified_unqualified_redundant() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) => x,
    Option.Some(y) => y,
    None => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_mixed_qualified_unqualified_non_exhaustive() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Option.Some(x) => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_result_never_err_exhaustive() {
    let input = r#"
enum Void {}

fn test(r: Result<int, Void>) -> int {
  match r {
    Result.Ok(n) => n,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_result_never_ok_exhaustive() {
    let input = r#"
enum Void {}

fn test(r: Result<Void, string>) -> string {
  match r {
    Result.Err(e) => e,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_result_never_both_exhaustive() {
    let input = r#"
enum Void {}

fn test(r: Result<Void, Void>) -> int {
  match r {
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_option_never_exhaustive() {
    let input = r#"
enum Void {}

fn test(o: Option<Void>) -> int {
  match o {
    Option.None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_either_never_left_exhaustive() {
    let input = r#"
enum Either<L, R> {
  Left(L),
  Right(R),
}

enum Void {}

fn test(e: Either<Void, int>) -> int {
  match e {
    Either.Right(n) => n,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_custom_enum_with_never_field() {
    let input = r#"
enum Command<E> {
  Run(string),
  Quit,
  Error(E),
}

enum Void {}

fn test(cmd: Command<Void>) -> string {
  match cmd {
    Command.Run(s) => s,
    Command.Quit => "quit",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_nested_never_in_struct() {
    let input = r#"
enum Void {}

struct Wrapper<T> {
  value: T,
}

fn test(o: Option<Wrapper<Never>>) -> int {
  match o {
    Option.None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_recursive_list_with_never() {
    let input = r#"
enum List<T> {
  Cons(T, List<T>),
  Nil,
}

enum Void {}

fn test(list: List<Void>) -> int {
  match list {
    List.Nil => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_inhabited_still_requires_all_arms() {
    let input = r#"
fn test(r: Result<int, string>) -> int {
  match r {
    Result.Ok(n) => n,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_baseless_recursive_enum_uninhabited() {
    let input = r#"
enum Infinite {
  Loop(Infinite),
}

fn test(i: Infinite) -> int {
  match i {
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_mutually_recursive_with_base_inhabited() {
    let input = r#"
enum A {
  ToB(B),
  End,
}

enum B {
  ToA(A),
}

fn test(a: A) -> int {
  match a {
    A.ToB(_) => 1,
    A.End => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_unreachable_arm_with_never() {
    let input = r#"
enum Void {}

fn test(r: Result<int, Void>) -> int {
  match r {
    Result.Ok(n) => n,
    Result.Err(_) => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_struct_with_nested_slice_non_exhaustive() {
    let input = r#"
struct Container { items: Slice<int> }

fn test(c: Container) -> int {
  match c {
    Container { items: [x] } => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_struct_with_nested_slice_exhaustive() {
    let input = r#"
struct Container { items: Slice<int> }

fn test(c: Container) -> int {
  match c {
    Container { items: [] } => 0,
    Container { items: [x, ..] } => x,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_generic_enum_different_instantiations_in_tuple() {
    let input = r#"
enum Void {}

enum MyResult<T, E> {
  MyOk(T),
  MyErr(E),
}

fn test(pair: (MyResult<int, Void>, MyResult<int, string>)) -> int {
  match pair {
    (MyOk(a), MyOk(b)) => a + b,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_generic_enum_different_instantiations_exhaustive() {
    let input = r#"
enum Void {}

enum MyResult<T, E> {
  MyOk(T),
  MyErr(E),
}

fn test(pair: (MyResult<int, Void>, MyResult<int, string>)) -> int {
  match pair {
    (MyOk(a), MyOk(b)) => a + b,
    (MyOk(_), MyErr(_)) => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_with_uninhabited_field_exhaustive_with_rest() {
    let input = r#"
struct Uninhabited {
  n: Never,
  m: int,
}

fn test(s: Uninhabited) -> int {
  match s {
    Uninhabited { m, .. } => m,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_with_uninhabited_field_no_redundancy_check() {
    let input = r#"
struct Uninhabited {
  n: Never,
  m: int,
}

fn test(s: Uninhabited) -> int {
  match s {
    Uninhabited { m, .. } => m,
    Uninhabited { m: x, .. } => x,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_empty_match_on_never_exhaustive() {
    let input = r#"
fn test(n: Never) -> int {
  match n {}
}

fn main() {}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guarded_only_on_never_is_exhaustive() {
    let input = r#"
enum Void {}

fn test(n: Never) {
  match n {
    _ if false => (),
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_tuple_with_never_first_element_exhaustive_with_single_arm() {
    let input = r#"
fn test(t: (Never, bool)) -> int {
  match t {
    (_, true) => 1,
  }
}

fn main() {}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_tuple_with_never_second_element_exhaustive() {
    let input = r#"
fn test(t: (bool, Never)) -> int {
  match t {
    (true, _) => 1,
  }
}

fn main() {}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_tuple_with_uninhabited_enum_exhaustive() {
    let input = r#"
enum Empty {}

fn test(t: (Empty, int)) -> int {
  match t {
    (_, 0) => 1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_empty_match_on_inhabited_type_non_exhaustive() {
    let input = r#"
fn test(x: int) -> int {
  match x {}
}

fn main() {}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_empty_match_on_bool_non_exhaustive() {
    let input = r#"
fn test(b: bool) {
  match b {}
}

fn main() {}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_guarded_arm_does_not_count_for_exhaustiveness() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(x) if x > 0 => "positive",
    None => "none",
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_guarded_arm_with_unguarded_fallback_exhaustive() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(x) if x > 0 => "positive",
    Some(_) => "non-positive",
    None => "none",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guarded_arm_not_redundant_after_same_pattern() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(x) if x > 0 => "positive",
    Some(_) => "other",
    None => "none",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guarded_arm_can_be_redundant() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(_) => "any",
    Some(x) if x > 0 => "positive",
    None => "none",
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_multiple_guarded_arms_same_pattern() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(x) if x > 0 => "positive",
    Some(x) if x < 0 => "negative",
    Some(_) => "zero",
    None => "none",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guarded_wildcard_not_exhaustive() {
    let input = r#"
match 42 {
  x if x > 0 => "positive",
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_guarded_wildcard_with_unguarded_exhaustive() {
    let input = r#"
match 42 {
  x if x > 0 => "positive",
  _ => "other",
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_slice_of_never_empty_only_exhaustive() {
    let input = r#"
enum Void {}

fn test(items: Slice<Void>) -> int {
  match items {
    [] => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_nested_slices_uninhabited_element_no_nonempty_required() {
    let input = r#"
enum Void {}

fn test(xs: Slice<int>, ys: Slice<Void>) -> int {
  match (xs, ys) {
    ([], []) => 0,
    ([x, ..], []) => x,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guard_if_true_not_exhaustive() {
    let input = r#"
match 42 {
  _ if true => "ok",
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_guard_if_true_does_not_make_later_arm_redundant() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) if true => x + 1,
    Some(x) => x,  // NOT redundant - guards are not evaluated
    None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guard_if_true_catch_all_does_not_make_wildcard_redundant() {
    let input = r#"
match 1 {
  _ if true => 0,
  _ => 1,  // NOT redundant - guards are not evaluated
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_guard_with_fallback_is_exhaustive() {
    let input = r#"
match 42 {
  x if x > 0 => "positive",
  _ => "non-positive",
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_or_pattern_exhaustive_enum() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(c: Color) -> string {
  match c {
    Red | Green => "warm",
    Blue => "cool",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_or_pattern_non_exhaustive_enum() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(c: Color) -> string {
  match c {
    Red | Green => "warm",
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_or_pattern_exhaustive_with_wildcard() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | 2 | 3 => "small",
    _ => "other",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_or_pattern_non_exhaustive_literals() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | 2 | 3 => "small",
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_or_pattern_redundant() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(c: Color) -> string {
  match c {
    Red | Green => "warm",
    Red => "red",
    Blue => "cool",
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_or_pattern_redundant_after_wildcard() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    _ => "any",
    1 | 2 | 3 => "small",
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_or_pattern_all_alternatives_redundant() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(c: Color) -> string {
  match c {
    Red => "red",
    Green => "green",
    Blue => "blue",
    Red | Green => "covered",
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_or_pattern_exhaustive_bool() {
    let input = r#"
fn test(a: bool, b: bool) -> string {
  match (a, b) {
    (true, true) | (false, false) => "same",
    (true, false) | (false, true) => "different",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_or_pattern_exhaustive_option() {
    let input = r#"
fn test(res: Result<int, int>) -> int {
  match res {
    Result.Ok(x) | Result.Err(x) => x,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_or_pattern_with_guard_not_exhaustive() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | 2 | 3 if x > 0 => "small positive",
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_or_pattern_with_guard_and_fallback() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | 2 | 3 if x > 0 => "small positive",
    _ => "other",
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_exhaustive_all_variants() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  KeyPress { key: string },
  Close,
}

fn test(e: Event) -> int {
  match e {
    Event.Click { x, y } => x + y,
    Event.KeyPress { key } => 0,
    Event.Close => 1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_non_exhaustive_missing_variant() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  KeyPress { key: string },
  Close,
}

fn test(e: Event) -> int {
  match e {
    Event.Click { x, y } => x + y,
    Event.Close => 1,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_struct_variant_exhaustive_with_wildcard() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  KeyPress { key: string },
  Close,
}

fn test(e: Event) -> int {
  match e {
    Event.Click { x, y } => x + y,
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_exhaustive_with_partial_fields() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  Close,
}

fn test(e: Event) -> int {
  match e {
    Event.Click { x, .. } => x,
    Event.Close => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_redundant_pattern() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  Close,
}

fn test(e: Event) -> int {
  match e {
    Event.Click { x, y } => x + y,
    Event.Close => 0,
    Event.Click { x, .. } => x,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_struct_variant_mixed_tuple_and_struct() {
    let input = r#"
enum Shape {
  Circle(int),
  Rectangle { width: int, height: int },
}

fn test(s: Shape) -> int {
  match s {
    Shape.Circle(r) => r,
    Shape.Rectangle { width, height } => width * height,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_mixed_non_exhaustive() {
    let input = r#"
enum Shape {
  Circle(int),
  Rectangle { width: int, height: int },
}

fn test(s: Shape) -> int {
  match s {
    Shape.Circle(r) => r,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_struct_variant_nested_in_option() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  Close,
}

fn test(opt: Option<Event>) -> int {
  match opt {
    Option.Some(Event.Click { x, y }) => x + y,
    Option.Some(Event.Close) => 0,
    Option.None => -1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_nested_non_exhaustive() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  Close,
}

fn test(opt: Option<Event>) -> int {
  match opt {
    Option.Some(Event.Click { x, y }) => x + y,
    Option.None => -1,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_struct_variant_generic() {
    let input = r#"
enum Container<T> {
  Empty,
  Single { value: T },
  Pair { first: T, second: T },
}

fn test(c: Container<int>) -> int {
  match c {
    Container.Empty => 0,
    Container.Single { value } => value,
    Container.Pair { first, second } => first + second,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_generic_non_exhaustive() {
    let input = r#"
enum Container<T> {
  Empty,
  Single { value: T },
  Pair { first: T, second: T },
}

fn test(c: Container<int>) -> int {
  match c {
    Container.Empty => 0,
    Container.Single { value } => value,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_struct_variant_with_never_field_exhaustive() {
    let input = r#"
enum Void {}

enum MaybeNever {
  Value { n: int },
  Impossible { never: Never },
}

fn test(m: MaybeNever) -> int {
  match m {
    MaybeNever.Value { n } => n,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_variant_field_reorder_exhaustive() {
    let input = r#"
enum Event {
  Click { x: int, y: int },
  Close,
}

fn test(e: Event) -> int {
  match e {
    Event.Click { y, x } => x + y,
    Event.Close => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_partial_exhaustive_all_variants() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  match p {
    Partial.Ok(n) => n,
    Partial.Err(_) => 0,
    Partial.Both(n, _) => n,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_partial_non_exhaustive_missing_both() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  match p {
    Partial.Ok(n) => n,
    Partial.Err(_) => 0,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_partial_non_exhaustive_missing_err() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  match p {
    Partial.Ok(n) => n,
    Partial.Both(n, _) => n,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_partial_exhaustive_with_wildcard() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  match p {
    Partial.Ok(n) => n,
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_arm_against_interface_wildcard_not_redundant() {
    let input = r#"
interface Event {}
struct ClickEvent { x: int, y: int }

fn handle(event: Event) -> int {
  match event {
    ClickEvent { x, .. } => x,
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_multiple_struct_arms_against_interface_wildcard_not_redundant() {
    let input = r#"
interface Event {}
struct ClickEvent { x: int, y: int }
struct KeyEvent { key: string }

fn handle(event: Event) -> int {
  match event {
    ClickEvent { x, .. } => x,
    KeyEvent { .. } => 1,
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_arm_against_interface_without_wildcard_is_non_exhaustive() {
    let input = r#"
interface Event {}
struct ClickEvent { x: int, y: int }

fn handle(event: Event) -> int {
  match event {
    ClickEvent { x, .. } => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_struct_arm_against_type_alias_to_interface_wildcard_not_redundant() {
    let input = r#"
interface Event {}
type Msg = Event
struct ClickEvent { x: int, y: int }

fn handle(msg: Msg) -> int {
  match msg {
    ClickEvent { x, .. } => x,
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_arm_against_chained_type_alias_to_interface_wildcard_not_redundant() {
    let input = r#"
interface Event {}
type Msg = Event
type OuterMsg = Msg
struct ClickEvent { x: int, y: int }

fn handle(msg: OuterMsg) -> int {
  match msg {
    ClickEvent { x, .. } => x,
    _ => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_bare_struct_name_arm_against_interface_wildcard_not_redundant() {
    let input = r#"
interface Event {}
struct ClickEvent { x: int, y: int }
struct KeyEvent { key: string }

fn handle(e: Event) -> int {
  match e {
    ClickEvent { x, .. } => x,
    KeyEvent => 0,
    _ => -1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn test_struct_arm_against_type_alias_to_interface_without_wildcard_is_non_exhaustive() {
    let input = r#"
interface Event {}
type Msg = Event
struct ClickEvent { x: int, y: int }

fn handle(msg: Msg) -> int {
  match msg {
    ClickEvent { x, .. } => x,
  }
}
"#;
    infer(input).assert_exhaustiveness_error();
}

#[test]
fn test_redundant_string_pattern_raw_vs_escaped() {
    let input = r#"
fn classify(s: string) -> int {
  match s {
    r"a\nb" => 1,
    "a\\nb" => 2,
    _ => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_string_pattern_unicode_escape_vs_literal() {
    let input = r#"
fn classify(s: string) -> int {
  match s {
    "A" => 1,
    "\u{0041}" => 2,
    _ => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_string_pattern_hex_escape_vs_literal() {
    let input = r#"
fn classify(s: string) -> int {
  match s {
    "A" => 1,
    "\x41" => 2,
    _ => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_string_pattern_octal_escape_vs_literal() {
    let input = r#"
fn classify(s: string) -> int {
  match s {
    "A" => 1,
    "\101" => 2,
    _ => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}

#[test]
fn test_redundant_string_pattern_newline_spellings() {
    let input = r#"
fn classify(s: string) -> int {
  match s {
    "\n" => 1,
    "\u{000A}" => 2,
    _ => 0,
  }
}
"#;
    infer(input).assert_redundancy_error();
}
