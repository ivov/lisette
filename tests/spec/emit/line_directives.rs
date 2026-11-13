use crate::_harness::emit;
use crate::_harness::emit_with_sourcemap;

#[test]
fn emits_line_directive_for_function_definition() {
    let input = "fn foo() -> int { 42 }";
    let result = emit_with_sourcemap(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:1"),
        "Expected line directive in:\n{}",
        go_code
    );
}

#[test]
fn line_directive_reflects_actual_line_number() {
    let input = r#"
import "go:fmt"
fn main() {
  fmt.Println(1)
  fmt.Println(2)
}
"#;
    let result = emit_with_sourcemap(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:4"),
        "Expected line 4 directive in:\n{}",
        go_code
    );
    assert!(
        go_code.contains("//line src/test.lis:5"),
        "Expected line 5 directive in:\n{}",
        go_code
    );
}

#[test]
fn emits_line_directive_for_closure() {
    let input = r#"
fn main() {
  let f = || 42
}
"#;
    let result = emit_with_sourcemap(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:3"),
        "Expected line directive for closure in:\n{}",
        go_code
    );
}

#[test]
fn line_directive_includes_column() {
    let input = "import \"go:fmt\"\nfn main() { fmt.Println(1) }";
    let result = emit_with_sourcemap(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:2:13"),
        "Expected line:column directive in:\n{}",
        go_code
    );
}

#[test]
fn package_file_line_directive_uses_relative_path_not_doubled() {
    use crate::_harness::MockFileSystem;
    use crate::_harness::build::compile_project_files;
    use semantics::store::ENTRY_PACKAGE_ID;

    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_PACKAGE_ID,
        "main.lis",
        "import \"greet\"\n\nfn main() {\n  let _ = greet.value()\n}\n",
    );
    fs.add_file_with_display(
        "greet",
        "greet.lis",
        "src/greet/greet.lis",
        "pub fn value() -> int {\n  42\n}\n",
    );

    let files = compile_project_files(fs, "github.com/user/myproject", true);
    let greet = files
        .iter()
        .find(|f| f.name == "greet/greet.go")
        .unwrap_or_else(|| {
            let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
            panic!("greet/greet.go must be emitted; got: {names:?}")
        });
    let go = greet.to_go();

    assert!(
        go.contains("//line src/greet/greet.lis:"),
        "package file directive should use its relative path, got:\n{go}"
    );
    assert!(
        !go.contains("//line greet/src/greet/greet.lis"),
        "package file path must not be doubled, got:\n{go}"
    );
}

fn assert_sourcemap_only_adds_directives(input: &str) {
    emit::assert_sourcemap_only_adds_directives(input, &emit::emit(input).go_code());
}

#[test]
fn sourcemap_keeps_else_if_condition_setup() {
    assert_sourcemap_only_adds_directives(
        r#"
fn bump(n: int) -> int { n + 1 }

fn pick(xs: Slice<int>, n: int) -> int {
  if n > 3 {
    1
  } else if xs[n] > bump(n) {
    2
  } else {
    3
  }
}
"#,
    );
}

#[test]
fn sourcemap_keeps_cleanup_of_directed_statements() {
    assert_sourcemap_only_adds_directives(
        r#"
fn pick(xs: Slice<int>, i: int) -> int {
  match xs.get(i) { Some(value) => value, None => -1 }
}

fn first_positive(s: Slice<int>, seen: Channel<int>) -> Option<int> {
  s.find(|x| {
    seen.send(x)
    x > 0
  })
}
"#,
    );
}

#[test]
fn sourcemap_drops_bodies_that_hold_only_directives() {
    assert_sourcemap_only_adds_directives(
        r#"
import "go:fmt"

enum Color { Red, Green, Blue }

fn test(x: int) {
  if x > 10 {
    let y = 1;
  } else {
    let y = 2;
  }
}

fn describe(color: Color) {
  match color {
    Red => fmt.Println("red"),
    Green => fmt.Println("green"),
    _ => (),
  }
}
"#,
    );
}

fn assert_directly_under(go_code: &str, go_line: &str, lis_line: usize) {
    let lines: Vec<&str> = go_code.lines().collect();
    let index = lines
        .iter()
        .position(|line| line.trim().starts_with(go_line))
        .unwrap_or_else(|| panic!("`{go_line}` is not emitted:\n{go_code}"));
    let expected = format!("//line src/test.lis:{lis_line}:");
    assert!(
        index > 0 && lines[index - 1].starts_with(&expected),
        "`{go_line}` must sit directly under a directive for line {lis_line}, got:\n{go_code}"
    );
}

#[test]
fn every_go_line_of_a_lowered_assignment_sits_under_its_directive() {
    let input = r#"
fn place(m: Map<string, int>) -> Slice<int> {
  let found = m.get("a")
  let mut xs = [0, 0, 0]
  xs[found.unwrap_or(0)] = 9
  xs
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "found = 0", 5);
    assert_directly_under(&go_code, "base[found] = 9", 5);
}

#[test]
fn every_go_line_of_a_propagation_sits_under_its_directive() {
    let input = r#"
fn parse(s: string) -> Result<int, string> { Ok(s.length()) }

fn twice(s: string) -> Result<int, string> {
  let a = parse(s)?
  Ok(a + 1)
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "return check", 5);
    assert_directly_under(&go_code, "a := check.OkVal", 5);
}

#[test]
fn a_match_arm_binding_sits_under_the_arm_directive() {
    let input = r#"
enum Shape { Circle(int), Square(int) }

fn area(s: Shape) -> int {
  match s {
    Circle(r) => r * r * 3,
    Square(w) => w * w,
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "r := s.Circle", 6);
    assert_directly_under(&go_code, "w := s.Square", 7);
}

#[test]
fn a_lambda_argument_body_gets_its_own_directive() {
    let input = r#"
fn big(x: int) -> bool { x > 1 }

fn main() {
  let n = [1, 2, 3]
    .filter(|x| big(x))
    .length()
  let _ = n
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "if big(x) {", 6);
}

#[test]
fn a_function_literal_body_sits_under_a_directive() {
    let input = r#"
import "go:strconv"

fn main() {
  {
    let f = strconv.Atoi
    let _ = f("1")
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(
        &go_code,
        "return lisette.ResultFromPair[int](strconv.Atoi(arg0))",
        6,
    );
}

#[test]
fn an_else_if_line_sits_under_its_own_directive() {
    let input = r#"
import "go:fmt"

fn bump(n: int) -> int { n + 1 }

fn pick(n: int) {
  if n > 3 {
    fmt.Println(1)
  } else if bump(n) > 2 {
    fmt.Println(2)
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "} else if bump(n) > 2 {", 9);
}

#[test]
fn a_select_case_line_sits_under_its_arm_directive() {
    let input = r#"
import "go:fmt"

fn drain(ch: Channel<string>) {
  select {
    let Some(msg) = ch.receive() => {
      fmt.Println(msg)
    },
    _ => fmt.Println("empty"),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "case msg, ok := <-", 6);
    assert_directly_under(&go_code, "default:", 9);
}

#[test]
fn a_synthesized_method_after_a_function_names_its_declaration() {
    let input = r#"
fn first() -> int {
  let a = 1
  a + 1
}

#[equality]
struct Row { cell: int }
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "return r.cell == other.cell", 8);
}

#[test]
fn a_try_block_tail_sits_under_its_own_directive() {
    let input = r#"
fn one() -> Result<int, string> { Ok(1) }

fn tail(n: int) -> Result<int, string> {
  try {
    let a = one()?
    a / n
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(
        &go_code,
        "return lisette.MakeResultOk[int, string](a / n)",
        7,
    );
}

#[test]
fn a_lambda_block_tail_sits_under_its_own_directive() {
    let input = r#"
fn filtered(xs: Slice<int>) -> Slice<int> {
  xs.filter(|x| {
    let a = 1
    a / x > 0
  })
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "tmp = a/x > 0", 5);
}

#[test]
fn a_select_send_case_line_sits_under_its_arm_directive() {
    let input = r#"
fn send(ch: Channel<int>) {
  select {
    ch.send(1) => {
      let _ = 2
    },
    _ => (),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "case ch <- 1:", 4);
}

#[test]
fn a_lowered_else_if_condition_sits_under_its_own_directive() {
    let input = r#"
import "go:fmt"

fn pair() -> Option<int> { Some(1) }

fn chain(n: int) {
  if n > 3 {
    fmt.Println(1)
  } else if pair().is_some() && pair().is_some() && 1 / n > 0 {
    fmt.Println(2)
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "if _, ok := pair(); ok && 1/n > 0 {", 9);
}

#[test]
fn a_match_test_after_a_guard_sits_under_its_arm_directive() {
    let input = r#"
fn guard(n: int, d: int) -> int {
  match n {
    1 => 10,
    x if 1 / d > x => {
      x
    },
    2 => 20,
    _ => 0,
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "if n == 2 {", 8);
}

#[test]
fn nested_function_literals_get_directives_in_linear_time() {
    let mut input = String::from("type A0 = int\n");
    for n in 1..=40 {
        input.push_str(&format!("type A{n} = Slice<A{}>\n", n - 1));
    }
    input.push_str("fn copy(x: A40) -> A40 {\n  x.clone()\n}\n");
    let go_code = emit_with_sourcemap(&input).go_code();
    let lines: Vec<&str> = go_code.lines().collect();
    let index = lines
        .iter()
        .position(|line| line.contains("return slices.Clone(e39)"))
        .unwrap_or_else(|| panic!("`return slices.Clone(e39)` is not emitted:\n{go_code}"));
    let directive = lines[..index]
        .iter()
        .rev()
        .find(|line| line.starts_with("//line "));
    assert!(
        directive.is_some_and(|line| line.starts_with("//line src/test.lis:43:")),
        "`return slices.Clone(e39)` must sit under a directive for line 43, got:\n{go_code}"
    );
}

#[test]
fn a_result_branch_assignment_sits_under_its_branch_directive() {
    let input = r#"
fn res(n: int, c: bool) -> Result<int, string> {
  let r: Result<int, string> = if c {
    Ok(1)
  } else {
    Ok(1 / n)
  }
  r
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "r = lisette.MakeResultOk[int, string](1 / n)", 6);
}

#[test]
fn a_grouped_match_guard_sits_under_its_arm_directive() {
    let input = r#"
fn groups(n: int) -> int {
  match n {
    0 if 1 / n > 0 => 1,
    0 => 2,
    1 if 2 / n > 0 => 3,
    1 => 4,
    _ => 5,
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "if 1/n > 0 {", 4);
    assert_directly_under(&go_code, "if n == 1 {", 6);
    assert_directly_under(&go_code, "if 2/n > 0 {", 6);
}

#[test]
fn a_guard_after_hoisted_bindings_sits_under_its_arm_directive() {
    let input = r#"
fn hoisted(o: Option<int>) -> int {
  match o {
    Some(x) if 1 / x > 0 => x,
    Some(x) if 2 / x > 0 => x + 1,
    Some(x) => x + 2,
    None => 0,
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "if 2/x > 0 {", 5);
}

#[test]
fn a_select_send_case_line_names_the_send_not_the_body() {
    let input = r#"
fn send(ch: Channel<int>) {
  select {
    ch.send(1) =>
      {
        let _ = 2
      },
    _ => (),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "case ch <- 1:", 4);
}

#[test]
fn a_value_switch_case_line_names_its_pattern() {
    let input = r#"
fn ints(n: int) -> int {
  match n {
    0 =>
      10,
    1 => 11,
    _ =>
      12,
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "case 0:", 4);
    assert_directly_under(&go_code, "default:", 7);
}

#[test]
fn a_bare_result_arm_assignment_sits_under_its_arm_directive() {
    let input = r#"
fn arm(n: int) -> Result<int, string> {
  let r: Result<int, string> = match n {
    0 => Err("zero"),
    _ => Ok(1 / n),
  }
  r
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(
        &go_code,
        "r = lisette.MakeResultErr[int, string](\"zero\")",
        4,
    );
}

#[test]
fn a_select_send_value_sits_under_its_case_directive() {
    let input = r#"
import "go:strconv"

fn send(ch: Channel<fn(string) -> Result<int, error>>) {
  select {
    ch.send(strconv.Atoi) => (),
    _ => (),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(
        &go_code,
        "return lisette.ResultFromPair[int](strconv.Atoi(arg0))",
        6,
    );
}

#[test]
fn a_select_match_receive_case_line_names_the_receive() {
    let input = r#"
fn recv(chans: Slice<Channel<int>>) {
  select {
    match chans[0].receive() {
      Some(v) => {
        let _ = v
      },
      None => (),
    },
    _ => (),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "case v, ok := <-chans[0]:", 4);
}

#[test]
fn a_hoisted_select_operand_sits_under_its_arm_directive() {
    let input = r#"
fn denominator() -> int { 0 }

fn relay(input: Channel<int>, out: Channel<int>) {
  select {
    let Some(v) = input.receive() => {
      let _ = v
    },
    out.send(1 / denominator()) => (),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "sendVal := 1 / denominator()", 9);
    assert_directly_under(&go_code, "for {", 5);
}

#[test]
fn a_select_default_line_names_the_wildcard() {
    let input = r#"
fn idle(ch: Channel<int>) {
  select {
    let Some(v) = ch.receive() => {
      let _ = v
    },
    _ =>
      (),
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_directly_under(&go_code, "default:", 7);
}

#[test]
fn a_multiline_raw_string_tag_gets_no_directive_inside() {
    let input = r#"
#[tag("custom")]
struct User {
  #[tag(`custom:"first"
other:"second"`)]
  name: string,
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert!(
        go_code.contains("`custom:\"first\"\nother:\"second\"`"),
        "{go_code}"
    );
}

/// The column is checked only when a directive is directly in front of `go_text`.
fn assert_positioned(go_code: &str, lis_line: usize, lis_column: usize, go_text: &str) {
    let found = go_positions(go_code, go_text)
        .into_iter()
        .any(|(line, column)| {
            line == Some(lis_line) && column.is_none_or(|column| column == lis_column)
        });
    assert!(
        found,
        "Go must map `{go_text}` to src/test.lis:{lis_line}:{lis_column}, got:\n{go_code}"
    );
}

/// Go counts lines forward from each directive.
fn go_positions(go_code: &str, go_text: &str) -> Vec<(Option<usize>, Option<usize>)> {
    let line_and_column = |position: &str| {
        let mut parts = position.rsplitn(3, ':');
        let column = parts.next().and_then(|column| column.parse().ok());
        (parts.next().and_then(|line| line.parse().ok()), column)
    };
    let mut line = None;
    let mut found = Vec::new();
    for text in go_code.lines() {
        if let Some(directive) = text.strip_prefix("//line ") {
            line = line_and_column(directive).0;
            continue;
        }
        let mut current = line;
        let mut rest = text;
        loop {
            match (rest.find("/*line "), rest.find(go_text)) {
                (Some(start), text_at) if text_at.is_none_or(|text_at| start < text_at) => {
                    let end = start + rest[start..].find("*/").expect("a directive closes");
                    let (directive_line, column) = line_and_column(&rest[start + 7..end]);
                    current = directive_line;
                    rest = rest[end + 2..].trim_start();
                    if let Some(after) = rest.strip_prefix(go_text) {
                        found.push((current, column));
                        rest = after;
                    }
                }
                (_, Some(text_at)) => {
                    found.push((current, None));
                    rest = &rest[text_at + go_text.len()..];
                }
                _ => break,
            }
        }
        line = current.map(|line| line + 1);
    }
    found
}

#[test]
fn a_call_on_a_continuation_line_keeps_its_own_line() {
    let input = r#"
fn part() -> int { 1 }

fn main() {
  let total = part() +
    part()
  let _ = total
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 6, 5, "part()");
}

#[test]
fn a_call_argument_on_its_own_line_keeps_its_own_line() {
    let input = r#"
fn part() -> int { 1 }
fn add(a: int, b: int) -> int { a + b }

fn main() {
  let total = add(
    part(),
    part(),
  )
  let _ = total
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 7, 5, "part()");
    assert_positioned(&go_code, 8, 5, ", part()");
}

#[test]
fn a_struct_field_on_its_own_line_keeps_its_own_line() {
    let input = r#"
struct Point { x: int, y: int }
fn part() -> int { 1 }

fn main() {
  let p = Point {
    x: part(),
    y: part(),
  }
  let _ = p
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 7, 5, "x:");
    assert_positioned(&go_code, 8, 5, "y:");
}

#[test]
fn a_field_of_a_one_line_struct_literal_names_the_literal_line() {
    let input = r#"
struct Point { x: int, y: int }
fn part() -> int { 1 }

fn main() {
  let p = Point { x: 1, y: part() }
  let _ = p
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 6, 19, "x: 1");
    assert_positioned(&go_code, 6, 25, "y: part()");
}

#[test]
fn an_element_of_a_one_line_wide_slice_literal_names_the_literal_line() {
    let input = r#"
fn part(label: string) -> int { 1 }

fn main() {
  let xs = [part("the first element of a wide slice"), part("the second element of a wide slice")]
  let _ = xs
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 5, 13, "part(\"the first");
    assert_positioned(&go_code, 5, 56, "part(\"the second");
}

#[test]
fn an_entry_of_a_one_line_wide_map_literal_names_the_literal_line() {
    let input = r#"
fn part(label: string) -> int { 1 }

fn main() {
  let m = Map.from([("first key that is wide", part("a")), ("second key that is wide", part("b"))])
  let _ = m
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 5, 21, "\"first key");
    assert_positioned(&go_code, 5, 60, "\"second key");
}

#[test]
fn a_tuple_struct_field_names_the_call_line() {
    let input = r#"
struct Pair(int, int)
fn part() -> int { 1 }

fn main() {
  let p = Pair(1, part())
  let _ = p
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 6, 19, "F1: part()");
}

#[test]
fn a_range_bound_names_the_range_line() {
    let input = r#"
fn part() -> int { 1 }

fn main() {
  let r = 1..part()
  let _ = r
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 5, 14, "End: part()");
}

#[test]
fn an_argument_after_a_multiline_literal_counts_from_the_literal_end() {
    let input = r#"
struct Point { x: int, y: int }
fn part() -> int { 1 }
fn take(p: Point, n: int) -> int { n }

fn main() {
  let n = take(Point { x: 1, y: 2 }, part())
  let _ = n
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 7, 36, "part()");
}

#[test]
fn a_call_through_an_index_on_later_lines_names_the_call_line() {
    let input = r#"
fn one() -> int { 1 }

fn main() {
  let handlers = [one]
  let n = handlers[
    0
  ]()
  let _ = n
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 7, 5, "0]");
    assert_positioned(&go_code, 6, 11, "()");
}

#[test]
fn a_receiver_method_call_with_the_receiver_on_its_own_line_names_the_call_line() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn add(self, x: int) -> int { self.n + x }
}

fn main() {
  let c = Counter { n: 1 }
  let n = Counter.add(
    c,
    2,
  )
  let _ = n
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 11, 5, "c.add");
    assert_positioned(&go_code, 10, 11, "(");
}

#[test]
fn a_pipeline_step_takes_the_line_of_its_step() {
    let input = r#"
fn double(x: int) -> int { x * 2 }
fn inc(x: int) -> int { x + 1 }

fn main() {
  let n = 1
    |> double
    |> inc
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 8, 8, "inc(");
    assert_positioned(&go_code, 7, 8, "double(");
    assert!(
        go_positions(&go_code, "(")
            .iter()
            .all(|(line, _)| *line != Some(6)),
        "no call may take the line of the pipeline's first value, got:\n{go_code}"
    );
}

#[test]
fn a_pipeline_step_into_a_method_takes_the_line_of_its_step() {
    let input = r#"
struct Counter { n: int }
impl Counter { fn add(self, k: int) -> int { self.n + k } }

fn main() {
  let c = Counter { n: 1 }
  let m = c
    |> Counter.add(2)
  let _ = m
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 8, 8, "(2)");
}

#[test]
fn a_method_step_after_a_call_step_takes_the_line_of_its_step() {
    let input = r#"
struct Counter { n: int }
impl Counter { fn add(self, k: int) -> int { self.n + k } }
fn build(n: int) -> Counter { Counter { n: n } }

fn main() {
  let n = 1
    |> build
    |> Counter.add(2)
  let _ = n
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 9, 8, "(2)");
}

#[test]
fn a_pipeline_match_subject_takes_the_line_of_its_last_step() {
    let input = r#"
fn id(s: string) -> string { s }

fn main() {
  match "abc"
    |> id
    |> string.substring(9..) {
    "" => {},
    _ => {},
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 7, 8, "lisette.SubstringFrom(");
}

#[test]
fn sourcemap_keeps_layout_with_inline_directives() {
    assert_sourcemap_only_adds_directives(
        r#"
struct Point { x: int, y: int }
fn part(label: string) -> int { 1 }
fn take(p: Point, n: int) -> int { n }

fn main() {
  let total = part("a") +
    part("b")
  let p = Point { x: part("c"), y: part("d") }
  let xs = [part("the first element of a wide slice"), part("the second element of a wide slice")]
  let n = take(Point {
    x: 1,
    y: part("e"),
  }, part("f"))
  let _ = total + p.x + xs.length() + n
}
"#,
    );
}

#[test]
fn sourcemap_keeps_rewrites_of_continuation_line_operands() {
    assert_sourcemap_only_adds_directives(
        r#"
struct Counter { n: int }

impl Counter {
  fn bump(self: Ref<Counter>, x: int) -> int { self.n + x }
}

fn main() {
  let mut x = 1
  x = x +
    1
  let mut c = Counter { n: 1 }
  let n = Counter.bump(
    &c,
    2,
  )
  let _ = x + n
}
"#,
    );
}

#[test]
fn an_address_receiver_on_its_own_line_keeps_its_own_line() {
    let input = r#"
struct Counter { n: int }

impl Counter {
  fn bump(self: Ref<Counter>, x: int) -> int { self.n + x }
}

fn main() {
  let mut c = Counter { n: 1 }
  let n = Counter.bump(
    &c,
    2,
  )
  let _ = n
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 11, 6, "c.bump");
}

#[test]
fn a_match_arm_does_not_take_the_subject_directive() {
    let input = r#"
enum Shape { Circle(int), Square(int) }
struct Holder { shape: Shape }

fn area(h: Holder) -> int {
  match h.shape {
    Shape.Circle(r) => r * 3,
    Shape.Square(s) => s * s,
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 8, 24, "h.shape.Square");
}

#[test]
fn a_format_string_on_its_own_line_collapses_like_plain_output() {
    assert_sourcemap_only_adds_directives(
        r#"
import "go:fmt"

fn show(x: int) {
  fmt.Println(fmt.Sprintf(
    "%d", x))
}
"#,
    );
}

const ADAPTED_CALLBACK: &str = r#"
fn some<T>(x: T) -> Option<T> { Some(x) }
fn part(x: Ref<int>) -> Ref<int> { x }
fn take(f: fn(Ref<int>) -> Option<Ref<int>>, x: Ref<int>) -> bool { f(x).is_some() }
"#;

#[test]
fn an_argument_after_a_generated_callback_keeps_its_own_line() {
    let input = format!(
        "{ADAPTED_CALLBACK}
fn main() {{
  let x = 1
  let ok = take(
    some,
    part(&x),
  )
  let _ = ok
}}
"
    );
    let go_code = emit_with_sourcemap(&input).go_code();
    assert_positioned(&go_code, 10, 5, "part(&x)");
}

#[test]
fn an_argument_after_a_generated_callback_keeps_the_statement_line() {
    let input = format!(
        "{ADAPTED_CALLBACK}
fn main() {{
  let x = 1
  let ok = take(some, part(&x))
  let _ = ok
}}
"
    );
    let go_code = emit_with_sourcemap(&input).go_code();
    assert_positioned(&go_code, 8, 23, "part(&x)");
}

#[test]
fn a_guard_on_continuation_lines_keeps_the_early_return() {
    assert_sourcemap_only_adds_directives(
        r#"
interface Event {}
struct Click {}

fn touch() {}

fn run(e: Event, a: bool, b: bool) {
  match e {
    Click {} if a &&
      b => touch(),
    Click {} => { return },
    _ => { return },
  }
}
"#,
    );
}

#[test]
fn a_directive_inside_a_string_does_not_hide_a_real_one() {
    let input = r#"
import "go:fmt"
fn part() -> int { 1 }
fn main() {
  fmt.Println("/*line src/test.lis:6:1*/",
    part())
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert!(
        go_code.contains("/*line src/test.lis:6:5*/"),
        "`part()` must keep its directive, got:\n{go_code}"
    );
}

#[test]
fn a_negated_guard_keeps_the_position_of_its_continuation_line() {
    let input = r#"
interface Event {}
struct Click {}
fn probe() -> int { 1 }
fn touch() {}
fn run(e: Event, a: bool) {
  match e {
    Click {} if a &&
      probe() == 1 => touch(),
    Click {} => { return },
    _ => { return },
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 9, 7, "probe()");
}

#[test]
fn a_forwarded_return_keeps_the_line_of_a_continuation_argument() {
    let input = r#"
fn probe() -> int { 1 }
fn step(n: int) -> Result<(), string> { Ok(()) }
fn run() -> Result<(), string> {
  step(
    probe()
  )?
  Ok(())
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 6, 5, "probe()");
}

#[test]
fn the_sourcemap_check_keeps_directive_text_inside_strings() {
    for input in [
        r#"fn text() -> string { "/*line src/test.lis:1:1*/" }"#,
        r#"fn text() -> string { "/*line " }"#,
        "fn text() -> string { r\"/*line src/test.lis:1:1*/\n  /*line \" }",
    ] {
        emit::assert_sourcemap_only_adds_directives(input, &emit::emit(input).go_code());
    }
}

#[test]
fn a_returned_array_from_call_keeps_the_line_of_its_argument() {
    let input = r#"
fn source() -> Slice<int> { [1] }
fn convert() -> Option<Array<int, 1>> {
  Array.from(source())
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 4, 3, "(source())");
}

#[test]
fn a_folded_compound_assignment_keeps_the_line_of_its_operand() {
    let input = r#"
fn probe() -> int { 1 }
fn run() -> int {
  let mut x = 0
  x = (x +
    probe())
  x
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 6, 5, "probe()");
}

#[test]
fn a_removed_negation_keeps_the_line_of_its_operand() {
    let input = r#"
interface Event {}
struct Click {}
fn probe() -> bool { true }
fn touch() {}
fn run(e: Event, a: bool) {
  match e {
    Click {} if a &&
      !(
        probe()
      ) => touch(),
    Click {} => { return },
    _ => { return },
  }
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 10, 9, "probe()");
}

#[test]
fn an_argument_after_a_generated_callback_in_a_tuple_keeps_its_own_line() {
    let input = r#"
fn some<T>(x: T) -> Option<T> { Some(x) }
fn probe() -> int { 1 }
fn pair() -> (fn(Ref<int>) -> Option<Ref<int>>, int) {
  (some, probe())
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 5, 10, "probe()");
}

#[test]
fn a_while_let_binding_from_a_field_path_takes_the_line_of_its_use() {
    let input = r#"
enum Action { Run(fn() -> int), Stop }
struct Holder { action: Action }
fn run(h: Holder) -> int {
  while let Action.Run(f) = h.action {
    return f()
  }
  0
}
"#;
    let go_code = emit_with_sourcemap(input).go_code();
    assert_positioned(&go_code, 6, 5, "return h.action.Run()");
}
