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

#[test]
fn module_file_line_directive_uses_relative_path_not_doubled() {
    use crate::_harness::MockFileSystem;
    use crate::_harness::build::compile_project_files;
    use semantics::store::ENTRY_MODULE_ID;

    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
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
        "module file directive should use its relative path, got:\n{go}"
    );
    assert!(
        !go.contains("//line greet/src/greet/greet.lis"),
        "module file path must not be doubled, got:\n{go}"
    );
}
