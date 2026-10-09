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
