use crate::_harness::emit_with_debug_info;

#[test]
fn emits_line_directive_for_function_definition() {
    let input = "fn foo() -> int { 42 }";
    let result = emit_with_debug_info(input);
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
    let result = emit_with_debug_info(input);
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
    let result = emit_with_debug_info(input);
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
    let result = emit_with_debug_info(input);
    let go_code = result.go_code();
    assert!(
        go_code.contains("//line src/test.lis:1:13"),
        "Expected line:column directive in:\n{}",
        go_code
    );
}
