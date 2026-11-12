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
fn main() {
  let x = 1
  let y = 2
}
"#;
    let result = emit_with_sourcemap(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:3"),
        "Expected line 3 directive in:\n{}",
        go_code
    );
    assert!(
        go_code.contains("//line src/test.lis:4"),
        "Expected line 4 directive in:\n{}",
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
    let input = "fn main() { let x = 1 }";
    let result = emit_with_sourcemap(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:1:13"),
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
    use crate::_harness::emit::emit;

    let plain = emit(input).go_code();
    let mapped = emit_with_sourcemap(input).go_code();
    let stripped: String = mapped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//line "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(
        stripped.trim_end(),
        plain.trim_end(),
        "sourcemap output differs beyond its directives:\n{mapped}"
    );
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
    assert_directly_under(&go_code, "return slices.Clone(e39)", 43);
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
