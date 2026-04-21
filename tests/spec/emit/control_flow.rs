use crate::assert_emit_snapshot;

#[test]
fn if_without_else() {
    let input = r#"
fn test(x: int) {
  if x > 10 {
    let y = 1;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_with_else() {
    let input = r#"
fn test(x: int) {
  if x > 10 {
    let y = 1;
  } else {
    let y = 2;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_returning_value() {
    let input = r#"
fn test(x: int) -> int {
  if x > 10 { 20 } else { 5 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_with_else_if() {
    let input = r#"
fn test(x: int) -> int {
  if x > 10 {
    100
  } else if x > 5 {
    50
  } else {
    10
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_nested_expression() {
    let input = r#"
fn test(x: int, y: int) -> int {
  if x > 0 {
    if y > 0 {
      100
    } else {
      50
    }
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_expression_in_let() {
    let input = r#"
fn test(x: int) -> int {
  let result = if x > 0 { x * 2 } else { 0 };
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_statement() {
    let input = r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let y = x + 1;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_with_else() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  if let Some(x) = opt {
    x * 2
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_in_let_binding() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let result = if let Some(x) = opt { x } else { 0 };
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_else_if_let() {
    let input = r#"
fn test(a: Option<int>, b: Option<int>) -> int {
  if let Some(x) = a {
    x
  } else if let Some(y) = b {
    y
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_irrefutable_with_else() {
    let input = r#"
fn test(x: int) -> int {
  if let y = x {
    y * 2
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_enum() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(color: Color) -> int {
  match color {
    Red => 1,
    Green => 2,
    Blue => 3,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_deref_enum() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(c: Ref<Color>) -> int {
  match c.* {
    Red => 1,
    Green => 2,
    Blue => 3,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_with_wildcard() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(color: Color) -> int {
  match color {
    Red => 1,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_with_enum_data() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) => x,
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_bool() {
    let input = r#"
fn test(flag: bool) -> int {
  match flag {
    true => 1,
    false => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_rune() {
    let input = r#"
fn test(c: rune) -> int {
  match c {
    'a' => 1,
    'b' => 2,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_with_break() {
    let input = r#"
fn test() {
  loop {
    break;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_break_value_in_let() {
    let input = r#"
fn test() -> int {
  let x = loop {
    if true {
      break 42
    }
  };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_break_value_nested() {
    let input = r#"
fn test() -> int {
  let x = loop {
    let inner = loop {
      break 10
    };
    break inner + 1
  };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_break_value_stmt_nested_in_expr() {
    let input = r#"
fn test() -> int {
  let x = loop {
    loop { break 42 }
    break 0
  };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_break_value_nested_in_expr_loop() {
    let input = r#"
fn test() -> int {
  let x = loop {
    while true { break }
    break 0
  };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_value_in_match_as_expression_in_loop() {
    let input = r#"
fn test() -> int {
  let mut n = 0;
  loop {
    n = match n {
      5 => break n,
      _ => n + 1,
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn continue_in_if_expression() {
    let input = r#"
fn test() -> int {
  let mut sum = 0;
  for i in [1, 2, 3, 4, 5] {
    let v = if i % 2 == 0 {
      continue
    } else {
      i
    };
    sum += v;
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_in_match_as_expression_no_value() {
    let input = r#"
fn test() {
  let mut i = 0;
  while i < 100 {
    let v = match i >= 5 {
      true => break,
      false => i,
    };
    let _ = v;
    i += 1;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_loop() {
    let input = r#"
fn test(x: int) {
  let mut count = x;
  while count > 0 {
    count = count - 1;
  };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_basic() {
    let input = r#"
fn test(opt: Option<int>) {
  let mut current = opt;
  while let Some(x) = current {
    current = None;
    let _ = x;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_with_break() {
    let input = r#"
fn test(opt: Option<int>) {
  let mut current = opt;
  while let Some(x) = current {
    if x > 100 {
      break;
    }
    current = None;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_tuple_pattern() {
    let input = r#"
fn test(opt: Option<(int, int)>) {
  let mut current = opt;
  while let Some((a, b)) = current {
    current = None;
    let _ = a + b;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_result_pattern() {
    let input = r#"
fn test(res: Result<int, string>) {
  let mut current = res;
  while let Ok(value) = current {
    current = Err("done");
    let _ = value;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  let mut sum = 0;
  for item in items {
    sum = sum + item;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_with_continue() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  let mut sum = 0;
  for item in items {
    if item < 0 {
      continue;
    };
    sum = sum + item;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_loops() {
    let input = r#"
fn test(matrix: Slice<Slice<int>>) -> int {
  let mut count = 0;
  for row in matrix {
    for item in row {
      count = count + 1;
    };
  };
  count
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_with_wildcard() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  let mut count = 0;
  for _ in items {
    count = count + 1;
  };
  count
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_struct_pattern() {
    let input = r#"
struct Point { x: int, y: int }

fn test(points: Slice<Point>) -> int {
  let mut sum = 0;
  for Point { x, y } in points {
    sum = sum + x + y;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_tuple_pattern_map() {
    let input = r#"
fn test(m: Map<string, int>) -> int {
  let mut sum = 0;
  for (_, value) in m {
    sum = sum + value;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_map_both_wildcards() {
    let input = r#"
fn test(m: Map<string, int>) -> int {
  let mut count = 0;
  for (_, _) in m {
    count += 1;
  };
  count
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_enumerate() {
    let input = r#"
fn test(items: Slice<string>) {
  for (i, item) in items.enumerate() {
    let _ = i;
    let _ = item;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_enumerate_wildcard_index() {
    let input = r#"
fn test(items: Slice<string>) {
  for (_, item) in items.enumerate() {
    let _ = item;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_enumerate_both_wildcards() {
    let input = r#"
fn test(items: Slice<int>) {
  for _ in items.enumerate() {
    let x = 1;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_tuple_pattern_slice() {
    let input = r#"
import "go:fmt"

fn test(pairs: Slice<(int, string)>) {
  for (a, b) in pairs {
    fmt.Println(a, b)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_exclusive() {
    let input = r#"
fn test() -> int {
  let mut sum = 0;
  for i in 0..5 {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_inclusive() {
    let input = r#"
fn test() -> int {
  let mut sum = 0;
  for i in 0..=5 {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_from() {
    let input = r#"
fn test() -> int {
  let mut sum = 0;
  for i in 0.. {
    if i >= 5 {
      break;
    }
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_with_expressions() {
    let input = r#"
fn test(start: int, end: int) -> int {
  let mut sum = 0;
  for i in start..end {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_with_call() {
    let input = r#"
fn get_end() -> int { 5 }

fn test() -> int {
  let mut sum = 0;
  for i in 0..get_end() {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_from_call() {
    let input = r#"
fn get_range() -> Range<int> { 0..5 }

fn test() -> int {
  let mut sum = 0;
  for i in get_range() {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_stored_range_exclusive() {
    let input = r#"
fn test() -> int {
  let r = 0..5;
  let mut sum = 0;
  for i in r {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_stored_range_inclusive() {
    let input = r#"
fn test() -> int {
  let r = 0..=5;
  let mut sum = 0;
  for i in r {
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_stored_range_from() {
    let input = r#"
fn test() -> int {
  let r = 0..;
  let mut sum = 0;
  for i in r {
    if i >= 5 {
      break;
    }
    sum = sum + i;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_range_wildcard() {
    let input = r#"
fn do_work() {}

fn test() {
  for _ in 0..3 {
    do_work();
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_stored_range_mutation_captured() {
    let input = r#"
fn main() {
  let mut r = 0..3
  let mut n = 0
  for i in r {
    n += 1
    r = 0..0
    let _ = i
  }
  let _ = n
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_inline_range_end_mutation_captured() {
    let input = r#"
fn main() {
  let mut n = 3
  let mut iters = 0
  for i in 0..n {
    iters += 1
    n = 0
    let _ = i
  }
  let _ = iters
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn early_return() {
    let input = r#"
fn test(x: int) -> int {
  if x < 0 {
    return 0;
  }
  x * 2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn multiple_return_points() {
    let input = r#"
fn test(x: int) -> int {
  if x < 0 {
    return 0;
  }
  if x > 100 {
    return 100;
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_tuple_simple() {
    let input = r#"
fn test() -> int {
  let pair = (1, 2);
  match pair {
    (0, 0) => 0,
    (1, 2) => 3,
    (x, y) => x + y,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_tuple_with_wildcard() {
    let input = r#"
fn test() -> int {
  let triple = (1, 2, 3);
  match triple {
    (1, _, _) => 1,
    (_, 2, _) => 2,
    (_, _, 3) => 3,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_tuple_nested() {
    let input = r#"
fn test() -> int {
  let nested = ((1, 2), 3);
  match nested {
    ((1, 2), 3) => 6,
    ((x, y), z) => x + y + z,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_multiple_with_same_vars() {
    let input = r#"
fn test() -> int {
  let pair = (10, 20);
  let first = match pair {
    (a, b) => a + b,
  };

  let nested = ((1, 2), (3, 4));
  let second = match nested {
    ((a, b), (c, d)) => a + b + c + d,
  };

  first + second
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_return_explicit() {
    let input = r#"
fn test() -> () {
  let x = 42;
  return ()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_return_implicit() {
    let input = r#"
fn test() -> () {
  let x = 42;
  ()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_return_early() {
    let input = r#"
fn test(flag: bool) -> () {
  if flag {
    return ()
  }
  let x = 10;
  ()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_struct_destructure() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> int {
  let p = Point { x: 10, y: 20 };
  let Point { x, y } = p;
  x + y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_struct_destructure_partial() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> int {
  let p = Point { x: 10, y: 20 };
  let Point { x, .. } = p;
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_slice_pattern_empty() {
    let input = r#"
fn test(items: Slice<int>) -> string {
  match items {
    [] => "empty",
    _ => "not empty",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_slice_pattern_fixed_length() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [a, b, c] => a + b + c,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_slice_pattern_with_rest() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [first, ..rest] => first,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_slice_pattern_rest_binding() {
    let input = r#"
fn len(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [first, ..rest] => 1 + len(rest),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_slice_pattern_discard_rest() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  match items {
    [] => 0,
    [first, ..] => first,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_struct_pattern() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> int {
  match p {
    Point { x: 0, y: 0 } => 0,
    Point { x, y } => x + y,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_struct_pattern_with_binding() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> int {
  match p {
    Point { x: a, y: b } => a * b,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_struct_rest_catchall() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> int {
  match p {
    Point { .. } => 1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_call_subject_catchall() {
    let input = r#"
struct Point { x: int, y: int }

fn make_point() -> Point {
  Point { x: 1, y: 2 }
}

fn test() -> int {
  match make_point() {
    Point { .. } => 1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn indexed_access_assignment() {
    let input = r#"
fn test() -> int {
  let mut xs = [1, 2, 3];
  xs[0] = 100;
  xs[0]
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn field_access_assignment() {
    let input = r#"
struct Point { x: int, y: int }

fn test() -> int {
  let mut p = Point { x: 1, y: 2 };
  p.x = 100;
  p.x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_with_block_value() {
    let input = r#"
fn test() -> int {
  let x = {
    let a = 10;
    let b = 20;
    a + b
  };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_with_select_expression() {
    let input = r#"
fn test() -> int {
  let ch = Channel.new<int>();
  let result = select {
    let Some(v) = ch.receive() => v,
    _ => 0,
  };
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_wildcard_pattern() {
    let input = r#"
fn compute() -> int { 42 }

fn test() {
  let _ = compute();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn enum_variant_with_multiple_fields_pattern() {
    let input = r#"
enum Pair<A, B> { Both(A, B), Neither }

fn test(p: Pair<int, string>) -> int {
  match p {
    Both(n, s) => n,
    Neither => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_basic() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let Some(x) = opt else { return 0; };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_with_break() {
    let input = r#"
fn test(opt: Option<int>) {
  loop {
    let Some(x) = opt else { break; };
    let _ = x;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_with_continue() {
    let input = r#"
fn test(items: Slice<Option<int>>) {
  for item in items {
    let Some(x) = item else { continue; };
    let _ = x;
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_result_pattern() {
    let input = r#"
fn test(res: Result<int, string>) -> int {
  let Ok(value) = res else { return -1; };
  value
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_irrefutable_pattern() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let x = opt else { return 0; };
  let _ = x;
  42
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_struct_pattern() {
    let input = r#"
struct Point { x: int, y: int }

fn test(opt: Option<Point>) -> int {
  let Some(Point { x, y }) = opt else { return 0; };
  x + y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_tuple_pattern() {
    let input = r#"
fn test(opt: Option<(int, int)>) -> int {
  let Some((a, b)) = opt else { return 0; };
  a + b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_or_pattern() {
    let input = r#"
enum E { A(int), B(int), C }

fn test(e: E) -> int {
  let A(x) | B(x) = e else { return 0; };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_tuple_or_pattern() {
    let input = r#"
fn test(pair: (Option<int>, Option<int>)) -> int {
  let (Some(x), Some(y)) | (Some(y), Some(x)) = pair else { return 0; };
  x + y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_struct_or_pattern() {
    let input = r#"
struct Point { x: Option<int>, y: Option<int> }

fn test(p: Point) -> int {
  let Point { x: Some(a), y: Some(b) } | Point { x: Some(b), y: Some(a) } = p else { return 0; };
  a + b
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_option_direct_call() {
    let input = r#"
fn get_value(s: string) -> Option<int> {
  if s == "ok" { Some(42) } else { None }
}

fn test() -> int {
  let Some(x) = get_value("ok") else { return 0; };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_result_direct_call() {
    let input = r#"
fn parse(s: string) -> Result<int, string> {
  if s == "42" { Ok(42) } else { Err("bad") }
}

fn test() -> int {
  let Ok(x) = parse("42") else { return -1; };
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_shadow_variable() {
    let input = r#"
fn test() -> int {
  let opt: Option<int> = Some(99)
  let Some(opt) = opt else { return 0; };
  opt
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn underscore_prefix_binding_discarded() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(_x) => "has value",
    None => "none",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_basic() {
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
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_with_binding() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(n) if n > 100 => n * 2,
    Some(n) if n > 0 => n,
    Some(_) => 0,
    None => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_on_struct() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) -> string {
  match p {
    Point { x, y } if x == y => "diagonal",
    Point { x, y } if x > y => "above",
    Point { x, y } => "below",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_mixed() {
    let input = r#"
fn test(opt: Option<int>) -> string {
  match opt {
    Some(x) if x > 0 => "positive",
    Some(_) => "non-positive",
    None => "none",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_tuple() {
    let input = r#"
fn test(pair: (int, int)) -> string {
  match pair {
    (x, y) if x == y => "equal",
    (x, y) if x > y => "greater",
    _ => "less",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_on_result_call() {
    let input = r#"
fn get_value() -> Result<int, string> { Ok(42) }

fn test() -> int {
  match get_value() {
    Ok(x) if x > 0 => x,
    Ok(_) => 0,
    Err(_) => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_on_option_call() {
    let input = r#"
fn get_option() -> Option<int> { Some(42) }

fn test() -> int {
  match get_option() {
    Some(x) if x > 0 => x,
    Some(_) => 0,
    None => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_literals() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | 2 | 3 => "small",
    4 | 5 | 6 => "medium",
    _ => "large",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_enum_variants() {
    let input = r#"
enum Color { Red, Green, Blue }

fn test(c: Color) -> string {
  match c {
    Red | Green => "warm",
    Blue => "cool",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_with_bindings() {
    let input = r#"
enum E { A(int), B(int) }

fn test(e: E) -> int {
  match e {
    A(x) | B(x) => x,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_strings() {
    let input = r#"
fn test(s: string) -> int {
  match s {
    "yes" | "y" | "true" => 1,
    "no" | "n" | "false" => 0,
    _ => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_booleans() {
    let input = r#"
fn test(a: bool, b: bool) -> string {
  match (a, b) {
    (true, true) | (false, false) => "same",
    (true, false) | (false, true) => "different",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_no_bindings() {
    let input = r#"
enum Status { Pending, Running, Complete, Failed }

fn test(s: Status) -> string {
  match s {
    Pending | Running => "in progress",
    Complete | Failed => "finished",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_in_if_let() {
    let input = r#"
fn test(x: int) -> string {
  if let 1 | 2 | 3 = x {
    "small"
  } else {
    "large"
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_in_while_let() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  let mut current = opt;
  let mut sum = 0;
  while let Some(1) | Some(2) | Some(3) = current {
    sum = sum + 1;
    current = None;
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_in_while_let_with_bindings() {
    let input = r#"
enum E { A(int), B(int) }

fn test() -> int {
  let mut current: Option<E> = Some(E.A(5));
  let mut sum = 0;
  while let Some(A(x)) | Some(B(x)) = current {
    sum = sum + x;
    current = None;
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_with_or_pattern_bindings() {
    let input = r#"
enum E { A(int), B(int), C(int) }

fn test(e: E) -> int {
  match e {
    C(x) if x > 0 => x * 100,
    A(x) | B(x) | C(x) => x,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_guard_evaluated_once() {
    let input = r#"
enum Res { Ok(int), Err(int) }

fn test(r: Res, threshold: int) -> int {
  match r {
    Ok(x) | Err(x) if x > threshold => 100,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_catchall_any_alternative() {
    let input = r#"
fn test(x: int) -> string {
  match x {
    1 | _ => "matched",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn or_pattern_exhaustive_with_bindings() {
    let input = r#"
enum Shape {
  Circle(int),
  Square(int),
  Triangle(int),
}

fn shape_size(s: Shape) -> int {
  match s {
    Shape.Circle(r) | Shape.Square(r) | Shape.Triangle(r) => r,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_basic_result() {
    let input = r#"
fn risky() -> Result<int, string> {
  Ok(42)
}

fn test() -> int {
  let result: Result<int, string> = try {
    risky()?
  };
  match result {
    Ok(x) => x,
    Err(_) => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_multiple_question_marks() {
    let input = r#"
fn get_a() -> Result<int, string> { Ok(10) }
fn get_b() -> Result<int, string> { Ok(20) }

fn test() -> int {
  let result = try {
    let a = get_a()?;
    let b = get_b()?;
    a + b
  };
  match result {
    Ok(sum) => sum,
    Err(_) => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_with_semicolon() {
    let input = r#"
fn do_thing() -> Result<int, string> { Ok(1) }

fn test() {
  let result: Result<int, string> = try {
    do_thing()?
  };
  let _ = result;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_option() {
    let input = r#"
fn maybe_get() -> Option<int> {
  Some(42)
}

fn test() -> int {
  let opt: Option<int> = try {
    let x = maybe_get()?;
    x + 1
  };
  match opt {
    Some(v) => v,
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_early_exit_with_err() {
    let input = r#"
fn test(bad: bool) -> int {
  let result: Result<int, string> = try {
    if bad {
      Err("failed")?
    }
    42
  };
  match result {
    Ok(x) => x,
    Err(_) => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_nested() {
    let input = r#"
fn risky_a() -> Result<int, string> { Ok(1) }
fn risky_b() -> Result<int, string> { Ok(2) }

fn test() -> int {
  let outer = try {
    let inner: Result<int, string> = try {
      risky_a()?
    };
    let inner_val = match inner {
      Ok(x) => x,
      Err(_) => 0,
    };
    inner_val + risky_b()?
  };
  match outer {
    Ok(x) => x,
    Err(_) => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_loop_inside() {
    let input = r#"
fn get_items() -> Result<Slice<int>, string> { Ok([1, 2, 3]) }

fn test() -> int {
  let result = try {
    let items = get_items()?;
    let mut sum = 0;
    for item in items {
      if item > 10 {
        break;
      }
      sum = sum + item;
    };
    sum
  };
  match result {
    Ok(s) => s,
    Err(_) => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_diverging_final_expression() {
    let input = r#"
fn risky() -> Result<int, string> { Ok(1) }

fn test() {
  let result: Result<int, string> = try {
    let _ = risky()?;
    loop {}
  };
  let _ = result;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_task_as_statement() {
    let input = r#"
fn risky() -> Result<int, string> { Ok(1) }
fn do_something() { }

fn test() {
  let r = try {
    let x = risky()?;
    task { do_something() };
    x
  };
  let _ = r;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn try_block_nested_result_final_expression() {
    let input = r#"
fn risky() -> Result<int, string> { Ok(1) }
fn nested() -> Result<int, string> { Ok(1) }

fn test() -> Result<Result<int, string>, string> {
  let r = try {
    let _ = risky()?;
    nested()
  };
  r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_statement_position_with_unit_arms() {
    let input = r#"
import "go:fmt"

fn test() {
  let result: Result<int, string> = Ok(42);
  match result {
    Ok(n) => fmt.Print(f"success: {n}\n"),
    Err(e) => fmt.Print(f"error: {e}\n"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_statement_position_with_guards() {
    let input = r#"
import "go:fmt"

fn test() {
  let result: Result<int, string> = Ok(42);
  match result {
    Ok(n) if n > 0 => fmt.Print(f"positive: {n}\n"),
    Ok(n) => fmt.Print(f"non-positive: {n}\n"),
    Err(e) => fmt.Print(f"error: {e}\n"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_with_integer_literal_and_catchall() {
    let input = r#"
import "go:fmt"

fn main() {
  for i in 0..10 {
    match i {
      d if d < 5 => fmt.Println("low"),
      5 =>          fmt.Println("five"),
      d =>          fmt.Println("high"),
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_with_guards_all_arms_diverge() {
    let input = r#"
fn main() {
  match 0 {
    _ if true => {return},
    _ => {panic("")}
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_statement_position_followed_by_statement() {
    let input = r#"
import "go:fmt"

fn test() {
  let result: Result<int, string> = Ok(42);
  match result {
    Ok(n) => fmt.Print(f"success: {n}\n"),
    Err(e) => fmt.Print(f"error: {e}\n"),
  }
  fmt.Print("done\n");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn underscore_prefixed_let_binding() {
    let input = r#"
import "go:fmt"

fn test() {
  let s = "hello, world";
  let _unused = s;
  fmt.Print("Done\n");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_without_else_branch() {
    let input = r#"
import "go:fmt"

fn divide(a: int, b: int) -> Option<int> {
  if b == 0 { None } else { Some(a / b) }
}

fn test() {
  if let Some(x) = divide(100, 10) {
    let _ = fmt.Print(f"Result: {x}\n");
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_result_without_else_branch() {
    let input = r#"
import "go:fmt"

fn try_parse(s: string) -> Result<int, string> {
  if s == "42" { Ok(42) } else { Err("not 42") }
}

fn test() {
  if let Ok(x) = try_parse("42") {
    let _ = fmt.Print(f"Parsed: {x}\n");
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_else_branch_with_statements_before_result_return() {
    let input = r#"
fn checked_sqrt(n: int) -> Result<int, string> {
  if n < 0 {
    Err("negative input")
  } else {
    let mut x = n;
    while x * x > n {
      x = x - 1;
    }
    Ok(x)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_arm_with_return_err_assigned_to_variable() {
    let input = r#"
fn safe_divide(a: float64, b: float64) -> Option<float64> {
  if b == 0.0 { None } else { Some(a / b) }
}

fn parse_and_divide(a: float64, b: float64) -> Result<string, string> {
  let result = match safe_divide(a, b) {
    Some(v) => v,
    None => return Err("division by zero"),
  };
  Ok(f"{result}")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn underscore_prefixed_let_with_option_result_call() {
    let input = r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn test() {
  let _unused = get_result();
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_scope_with_variable_shadowing() {
    let input = r#"
fn test() -> string {
  let x = "outer"
  {
    let x = "inner"
    let _ = x
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_variable_does_not_shadow_outer_binding() {
    let input = r#"
fn test() -> int {
  let i = 100
  for i in 0..3 {
    let _ = i
  }
  i
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_let_shadowing_does_not_leak() {
    let input = r#"
fn test() -> string {
  let x = "outside"
  for i in 0..3 {
    let x = i
    let _ = x
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn block_expression_shadowing_does_not_leak() {
    let input = r#"
fn test() -> int {
  let y = 10
  let z = {
    let y = 20
    y
  }
  let _ = z
  y
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_tuple_all_unused_bindings() {
    let input = r#"
fn test() {
  let (_a, _b) = (1, 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn underscore_prefixed_binding_used() {
    let input = r#"
fn test() -> int {
  let _x = 42;
  _x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn underscore_prefixed_binding_unused() {
    let input = r#"
fn test() {
  let _x = 42;
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn underscore_prefixed_match_arm_binding_used() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(_val) => _val + 1,
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_arm_index_assignment() {
    let input = r#"
fn test(key: string, value: int) {
  let mut m = Map.new()
  match key {
    "a" => m[key] = value,
    _ => m["default"] = 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_in_if_block() {
    let input = r#"
import "go:fmt"

fn test(flag: bool) {
  if flag {
    defer fmt.Println("cleanup")
    fmt.Println("work")
  }
  fmt.Println("after if")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_in_match_in_loop() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  let mut sum = 0
  for item in items {
    match item {
      0 => break,
      n => sum += n,
    }
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn continue_in_match_in_loop() {
    let input = r#"
fn test(items: Slice<int>) -> int {
  let mut sum = 0
  for item in items {
    match item {
      0 => continue,
      n => sum += n,
    }
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn continue_in_guarded_match_in_loop() {
    let input = r#"
fn sum_positives(items: Slice<int>) -> int {
  let mut sum = 0
  for item in items {
    match item {
      n if n > 0 => sum += n,
      _ => continue,
    }
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_in_guarded_match_in_loop() {
    let input = r#"
fn find_first_negative(items: Slice<int>) -> int {
  let mut result = 0
  for item in items {
    match item {
      n if n < 0 => { result = n; break },
      _ => (),
    }
  }
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_struct_literal_in_condition() {
    let input = r#"
struct Point { x: int, y: int }

impl Point {
  fn sum(self) -> int { self.x + self.y }
}

fn test() -> int {
  if Point { x: 1, y: 2 }.sum() > 0 {
    1
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_generic_struct_literal_in_condition() {
    let input = r#"
struct Box<T> { v: T }

fn test() -> int {
  let b = Box { v: 1 }
  if b == Box { v: 2 } {
    1
  } else {
    0
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_generic_struct_literal_selector() {
    let input = r#"
struct Box<T> { v: T }

fn test() -> int {
  if Box { v: 1 }.v != 1 {
    0
  } else {
    1
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_generic_tuple_struct_comparison() {
    let input = r#"
struct W<T>(T)

fn test() -> int {
  let w = W(1)
  if w == W(2) { 0 } else { 1 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_generic_tuple_selector() {
    let input = r#"
struct W<T>(T)

fn test() -> int {
  match 1 {
    n if W(1).0 == 1 => n,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_generic_tuple_comparison() {
    let input = r#"
struct W<T>(T)

fn test() -> int {
  let w = W(1)
  match w {
    x if x == W(2) => 0,
    _ => 1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_generic_tuple_selector() {
    let input = r#"
struct W<T>(T)

fn test() {
  let mut i = 0
  while W(1).0 == 1 && i < 1 {
    i = i + 1
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guarded_catchall_unused_subject() {
    let input = r#"
struct P { v: int }
fn mk() -> P { P { v: 1 } }

fn test() {
  match mk() {
    P { v: _ } if false => (),
    _ => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_irrefutable_unused_subject() {
    let input = r#"
struct P { v: int }
fn mk() -> P { P { v: 1 } }

fn test() {
  if let P { v: _ } = mk() {
    ()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_irrefutable_unused_subject() {
    let input = r#"
struct P { v: int }
fn mk() -> P { P { v: 1 } }

fn test() {
  let P { v: _ } = mk() else {
    panic("no")
  }
  ()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_in_loop_receiver_collision() {
    let input = r#"
import "go:fmt"

struct IntList {
  items: Slice<int>,
}

impl IntList {
  fn display(self) -> string {
    let mut s = "["
    for i in self.items {
      if s != "[" {
        s = s + ", "
      }
      s = s + f"{i}"
    }
    s + "]"
  }
}

fn main() {
  let list = IntList { items: [1, 2, 3] }
  fmt.Println(list.display())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_arm_shadow_does_not_clobber_result_var() {
    let input = r#"
fn test(x: int) -> string {
  let result = match x {
    n if n < 10 => {
      let result = n * n
      f"small({result})"
    },
    _ => "other",
  }
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_struct_literal_condition_parenthesized() {
    let input = r#"
struct Point { x: int, y: int }

fn test() {
  while Point { x: 1, y: 2 }.x > 0 {
    break
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_struct_literal_subject_parenthesized() {
    let input = r#"
import "go:fmt"

struct Point { x: int }

fn main() {
  let result = match Point { x: 1 }.x {
    1 => 10,
    _ => 0,
  }
  fmt.Println(result)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_label_for_break_in_let_else_inside_match() {
    let input = r#"
fn maybe(flag: bool) -> Option<int> {
  if flag { Some(42) } else { None }
}

fn test() -> int {
  let mut count = 0
  loop {
    count = count + 1
    if count > 5 { break }
    match count {
      1 => {
        let Some(v) = maybe(true) else { break }
        let _ = v
      },
      _ => {},
    }
  }
  count
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_loop_shadow_does_not_leak() {
    let input = r#"
fn test() -> string {
  let result = "final"
  let mut i = 0
  while i < 3 {
    let result = i * 10
    let _ = result
    i += 1
  }
  result
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_struct_literal() {
    let input = r#"
struct Point { x: int, y: int }

fn main() {
  let p = Point { x: 1, y: 2 }
  match p {
    _ if p == Point { x: 1, y: 2 } => { let _ = 1 },
    _ => { let _ = 2 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guard_switch_fallback_value_position() {
    let input = r#"
enum Dir { North, South }

fn check(d: Dir, urgent: bool) -> string {
  match d {
    Dir.North if urgent => "NORTH!",
    Dir.North => "north",
    Dir.South => "south",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_or_pattern_no_loop_wrapper() {
    let input = r#"
enum Val { A(int), B(int), C }

fn test(items: Slice<Val>) -> int {
  let mut sum = 0
  for item in items {
    let Val.A(n) | Val.B(n) = item else {
      continue
    }
    sum += n
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_unused_some_ok_check() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>()
  select {
    let Some(_v) = ch.receive() => {
      let _ = 1
    },
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_ref_call_channel_operand() {
    let input = r#"
fn get_channel(ch: Ref<Channel<int>>) -> Ref<Channel<int>> {
  ch
}

fn test(ch: Ref<Channel<int>>) {
  select {
    let Some(v) = get_channel(ch).receive() => {
      let _ = v
    },
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_ref_call_channel_operand() {
    let input = r#"
fn get_channel(ch: Ref<Channel<int>>) -> Ref<Channel<int>> {
  ch
}

fn test(ch: Ref<Channel<int>>) {
  select {
    match get_channel(ch).receive() {
      Some(v) => { let _ = v },
      None => {},
    },
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_on_enum_variant_as_subject() {
    let input = r#"
enum Dir { North, South, East }

fn test() -> string {
  match Dir.North {
    Dir.North => "north",
    Dir.South => "south",
    _ => "other",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_expression_shadow_same_name_binding() {
    let input = r#"
fn test() -> int {
  let x = loop {
    let x = 1
    break x
  }
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_binding_block_with_return_diverges() {
    let input = r#"
fn f() -> int {
  let x = {
    return 1
  }
  x
}

fn test() -> int {
  f()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_break_unit_call_value() {
    let input = r#"
fn noop() {}

fn test() {
  let _ = loop {
    break noop()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_propagate_channel_operand() {
    let input = r#"
fn g() -> Result<int, string> { Ok(0) }

fn f() -> Result<int, string> {
  let chans = [Channel.buffered<int>(1)]
  chans[0].send(7)
  let x = select {
    let Some(v) = chans[g()?].receive() => v,
    _ => 0,
  }
  Ok(x)
}

fn main() { let _ = f() }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_propagate_channel_call() {
    let input = r#"
fn g() -> Result<Channel<int>, string> { Ok(Channel.buffered<int>(1)) }

fn f() -> Result<int, string> {
  let ch = Channel.buffered<int>(1)
  ch.send(7)
  let x = select {
    match g()?.receive() {
      Some(v) => v,
      None => 0,
    },
    _ => 0,
  }
  Ok(x)
}

fn main() { let _ = f() }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_propagate_channel_call() {
    let input = r#"
fn g() -> Result<Channel<int>, string> { Ok(Channel.buffered<int>(1)) }

fn f() -> Result<int, string> {
  let ch = Channel.buffered<int>(1)
  ch.send(7)
  let x = select {
    let Some(v) = g()?.receive() => v,
    _ => 0,
  }
  Ok(x)
}

fn main() { let _ = f() }
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_subject_unit_returning_call() {
    let input = r#"
fn noop() {}

fn main() {
  let x = match noop() {
    () => 1,
  }
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_value_in_if_branch_of_loop_expr() {
    let input = r#"
fn test() {
  let _ = loop {
    let _ = if true { break 1 } else { 1 }
    2
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_block_continue_in_if_branch() {
    let input = r#"
fn test() {
  loop {
    let _ = if true { { continue } } else { { 1 } }
    break
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_block_continue_in_let() {
    let input = r#"
fn test() {
  loop {
    let _ = { { continue } }
    break
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_block_break_as_match_subject() {
    let input = r#"
fn test() {
  let _ = loop {
    let _ = match { { break 1 } } {
      1 => 1,
      _ => 2,
    }
    3
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_block_return_as_match_subject() {
    let input = r#"
fn test() -> int {
  let _ = match { { return 1 } } {
    1 => 1,
    _ => 2,
  }
  3
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_value_in_if_branch_assignment_rhs() {
    let input = r#"
fn test() {
  let _ = loop {
    let mut x = 0
    x = if true { break 1 } else { 1 }
    let _ = x
    2
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn break_value_in_match_branch_assignment_rhs() {
    let input = r#"
fn test() {
  let _ = loop {
    let mut x = 0
    x = match 1 { 1 => break 1, _ => 1, }
    let _ = x
    2
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_block_return_in_assignment_rhs() {
    let input = r#"
fn test() -> int {
  let mut x = 0
  x = { { return 1 } }
  let _ = x
  2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn nested_block_return_in_struct_spread() {
    let input = r#"
struct S { a: int }
fn test() -> int {
  let _ = S { a: 1, ..{ { return 1 } } }.a
  2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_with_user_never_branch_tail() {
    let input = r#"
fn die() -> Never { panic("dead") }

fn test() -> int {
  if true { die() } else { 1 }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_complex_pattern_unused_temp() {
    let input = r#"
struct P { v: int }
fn mk() -> P { P { v: 1 } }
fn test() {
  let P { v } = mk()
  ()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_single_arm_struct_unused_subject() {
    let input = r#"
struct P { v: int }
fn mk() -> P { P { v: 1 } }
fn test() {
  match mk() {
    P { v } => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_single_arm_struct_ident_subject_unused() {
    let input = r#"
struct P { v: int }
fn test() {
  let p = P { v: 1 }
  let _ = match p {
    P { v } => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_irrefutable_ident_subject_unused() {
    let input = r#"
struct P { v: int }
fn test() {
  let p = P { v: 1 }
  if let P { v } = p {
    ()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_guarded_ident_subject_unused() {
    let input = r#"
struct P { v: int }
fn test() {
  let p = P { v: 1 }
  match p {
    P { v } if false => (),
    _ => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_recover_unused_subject() {
    let input = r#"
struct P { v: int }
fn test() {
  let out = recover {
    let p = P { v: 1 }
    match p { P { v } => () }
  }
  let _ = out
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_defer_unused_subject() {
    let input = r#"
struct P { v: int }
fn test() {
  defer {
    let p = P { v: 1 }
    match p { P { v } => () }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_task_unused_subject() {
    let input = r#"
struct P { v: int }
fn test() {
  task {
    let p = P { v: 1 }
    match p { P { v } => () }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_with_user_never_arm_tail() {
    let input = r#"
fn die() -> Never { panic("dead") }

fn test(x: int) -> int {
  match x {
    0 => die(),
    _ => x + 1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_or_pattern_catchall_unused_subject() {
    let input = r#"
fn test() {
  let _ | _ = 1 else {
    panic("x")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_complex_pattern_unused_item() {
    let input = r#"
struct P { v: int }
fn test() {
  for P { v } in [P { v: 1 }] {
    ()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_enumerate_complex_all_unused() {
    let input = r#"
fn test() {
  let items = [(1,2), (3,4)]
  for (i, (a, b)) in items.enumerate() {
    ()
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_enumerate_complex_key_unused() {
    let input = r#"
fn test() -> int {
  let items = [(1,2), (3,4)]
  let mut sum = 0
  for (i, (a, b)) in items.enumerate() {
    sum = sum + a + b
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_enumerate_complex_value_unused() {
    let input = r#"
fn test() -> int {
  let items = [(1,2), (3,4)]
  let mut sum = 0
  for (i, (a, b)) in items.enumerate() {
    sum = sum + i
  }
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_in_lambda_unused_subject() {
    let input = r#"
struct P { v: int }
fn test() -> int {
  let opt = Some(1)
  let result = opt.map(|x| {
    let p = P { v: x }
    match p {
      P { v } => (),
    }
    1
  })
  match result {
    Some(x) => x,
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_wildcard_unused_subject() {
    let input = r#"
fn test() {
  let _ = 1 else { panic("x") }
  ()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_or_pattern_else_scope_shadow() {
    let input = r#"
enum E {
  A(int),
  B(int),
  C,
}
fn test() {
  let x = 5
  let e = E.C
  let E.A(x) | E.B(x) = e else {
    if x != 5 { panic("shadow") }
    return
  }
  let _ = x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_or_pattern_dropped_binding_else_leak() {
    let input = r#"
enum E {
  A(int),
  B(int),
  C,
}
fn test() {
  let x = 5
  let e = E.C
  let E.A(x) | E.B(x) = e else {
    if x != 5 { panic("shadow") }
    return
  }
  panic("unexpected")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_or_pattern_scope_leak_after() {
    let input = r#"
enum E {
  A(int),
  B(int),
  C,
}
fn test() {
  let x = 5
  let _ = x
  let e = E.A(9)
  let E.A(x) | E.B(x) = e else {
    return
  }
  if x != 5 { panic("shadow") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn if_let_or_pattern_unused_branch_binding() {
    let input = r#"
enum E { A(int), B(int), C }
fn test() {
  let x = 5
  let e = E.C
  if let E.A(x) | E.B(x) = e {
    panic("unexpected")
  } else {
    if x != 5 { panic("shadow") }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_or_pattern_unused_branch_binding() {
    let input = r#"
enum E { A(int), B(int), C }
fn test() {
  let x = 5
  let e = E.C
  match e {
    E.A(x) | E.B(x) => panic("unexpected"),
    _ => {
      if x != 5 { panic("shadow") }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_let_or_pattern_unused_branch_binding() {
    let input = r#"
enum E { A(int), B(int), C }
fn test() {
  let x = 5
  let e = E.C
  while let E.A(x) | E.B(x) = e {
    panic("unexpected")
  }
  if x != 5 { panic("shadow") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_or_pattern_guard_unused_binding() {
    let input = r#"
enum E { A(int), B(int), C }
fn test() {
  let e = E.C
  match e {
    E.A(x) | E.B(x) if false => (),
    _ => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_or_pattern_partial_unused_binding() {
    let input = r#"
enum E { A((int, int)), B((int, int)), C }
fn eval(e: E) -> int {
  match e {
    E.A((x, y)) | E.B((x, y)) => x,
    _ => 0,
  }
}
fn test() {
  if eval(E.B((4, 9))) != 4 { panic("bad") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn match_or_pattern_guard_partial_unused_binding() {
    let input = r#"
enum E { A((int, int)), B((int, int)), C }
fn eval(e: E) -> int {
  match e {
    E.A((x, y)) | E.B((x, y)) if x > 0 => x,
    _ => 0,
  }
}
fn test() {
  if eval(E.B((4, 9))) != 4 { panic("bad") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_condition_with_setup_statements_inside_loop() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = i.* + 1
  i.*
}
fn test() {
  let mut i = 0
  while (i < 3, bump(&i)).0 {
    let _ = i
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn while_binary_condition_with_capture_inside_loop() {
    let input = r#"
fn bump(i: Ref<int>) -> int {
  i.* = i.* + 1
  i.*
}
fn test() {
  let mut i = 0
  while i < 3 && bump(&i) > 0 {
    let _ = i
  }
}
"#;
    assert_emit_snapshot!(input);
}
