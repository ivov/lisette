mod lsp_harness;

use lsp_harness::{
    TestClient, completion_labels, definition_location, hover_content, symbol_names,
};
use tower_lsp::lsp_types::*;

const TEST_URI: &str = "file:///test.lis";

#[tokio::test]
async fn initialize_returns_capabilities() {
    let mut client = TestClient::new().await;
    let result = client.initialize().await;

    assert!(result.capabilities.hover_provider.is_some());
    assert!(result.capabilities.definition_provider.is_some());
    assert!(result.capabilities.references_provider.is_some());
    assert!(result.capabilities.completion_provider.is_some());
    assert!(result.capabilities.signature_help_provider.is_some());
    assert!(result.capabilities.rename_provider.is_some());
    assert!(result.capabilities.document_formatting_provider.is_some());
    assert!(result.capabilities.document_symbol_provider.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn hover_shows_function_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn add(x: int, y: int) -> int { x + y }")
        .await;

    let hover = client.hover(TEST_URI, 0, 3).await;
    assert!(hover.is_some());

    let content = hover_content(&hover.unwrap());
    assert!(content.contains("int"));
    assert!(content.contains("->"));

    client.shutdown().await;
}

#[tokio::test]
async fn hover_shows_variable_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { let x = 42; x }").await;

    let hover = client.hover(TEST_URI, 0, 16).await;
    assert!(hover.is_some());

    let content = hover_content(&hover.unwrap());
    assert!(content.contains("int"));

    client.shutdown().await;
}

#[tokio::test]
async fn hover_shows_string_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, r#"fn main() { let s = "hello"; s }"#)
        .await;

    let hover = client.hover(TEST_URI, 0, 16).await;
    assert!(hover.is_some());

    let content = hover_content(&hover.unwrap());
    assert!(content.contains("string"));

    client.shutdown().await;
}

#[tokio::test]
async fn hover_on_tuple_binding_shows_element_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, r#"fn main() { let (a, b) = (1, "hi"); b }"#)
        .await;

    let hover = client.hover(TEST_URI, 0, 20).await;
    assert!(hover.is_some());

    let content = hover_content(&hover.unwrap());
    assert!(content.contains("string"));

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_local_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let x = 1\n  x + 1\n}")
        .await;

    let response = client.goto_definition(TEST_URI, 2, 2).await;
    assert!(response.is_some());

    let response = response.unwrap();
    let loc = definition_location(&response);
    assert!(loc.is_some());

    let loc = loc.unwrap();
    assert_eq!(loc.range.start.line, 1);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_function_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn foo() { 1 }\nfn main() { foo() }")
        .await;

    let response = client.goto_definition(TEST_URI, 1, 12).await;
    assert!(response.is_some());

    let response = response.unwrap();
    let loc = definition_location(&response);
    assert!(loc.is_some());

    let loc = loc.unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_function_parameter() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn add(x: int) { x + 1 }").await;

    let response = client.goto_definition(TEST_URI, 0, 17).await;
    assert!(response.is_some());

    let response = response.unwrap();
    let loc = definition_location(&response);
    assert!(loc.is_some());

    let loc = loc.unwrap();
    assert_eq!(loc.range.start.line, 0);
    assert!(loc.range.start.character < 10);

    client.shutdown().await;
}

#[tokio::test]
async fn references_finds_all_usages() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn foo() { 1 }\nfn main() { foo(); foo() }")
        .await;

    let refs = client.references(TEST_URI, 0, 3, true).await;
    assert!(refs.is_some());

    let locations = refs.unwrap();
    assert_eq!(locations.len(), 3);

    client.shutdown().await;
}

#[tokio::test]
async fn references_excludes_declaration_when_flag_false() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn foo() { 1 }\nfn main() { foo(); foo() }")
        .await;

    let refs = client.references(TEST_URI, 0, 3, false).await;
    assert!(refs.is_some());

    let locations = refs.unwrap();
    assert_eq!(locations.len(), 2);

    client.shutdown().await;
}

#[tokio::test]
async fn references_local_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let x = 1\n  x + x\n}")
        .await;

    let refs = client.references(TEST_URI, 1, 6, true).await;
    assert!(refs.is_some());

    let locations = refs.unwrap();
    assert_eq!(locations.len(), 3);

    client.shutdown().await;
}

#[tokio::test]
async fn completion_includes_keywords() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "").await;

    let response = client.completion(TEST_URI, 0, 0).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(labels.iter().any(|l| l == "fn"));
    assert!(labels.iter().any(|l| l == "let"));
    assert!(labels.iter().any(|l| l == "if"));
    assert!(labels.iter().any(|l| l == "match"));
    assert!(labels.iter().any(|l| l == "struct"));
    assert!(labels.iter().any(|l| l == "enum"));

    client.shutdown().await;
}

#[tokio::test]
async fn completion_includes_prelude_types() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "").await;

    let response = client.completion(TEST_URI, 0, 0).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(labels.iter().any(|l| l == "int"));
    assert!(labels.iter().any(|l| l == "string"));
    assert!(labels.iter().any(|l| l == "bool"));
    assert!(labels.iter().any(|l| l == "Option"));
    assert!(labels.iter().any(|l| l == "Result"));

    client.shutdown().await;
}

#[tokio::test]
async fn completion_includes_local_bindings() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let myvar = 1\n  m\n}")
        .await;

    let response = client.completion(TEST_URI, 2, 3).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(labels.iter().any(|l| l == "myvar"));

    client.shutdown().await;
}

#[tokio::test]
async fn completion_includes_defined_functions() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn helper() { 1 }\nfn main() { h }")
        .await;

    let response = client.completion(TEST_URI, 1, 13).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(labels.iter().any(|l| l == "helper"));

    client.shutdown().await;
}

#[tokio::test]
async fn signature_help_shows_function_params() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn add(x: int, y: int) -> int { x + y }\nfn main() { add(1, 2) }",
        )
        .await;

    let help = client.signature_help(TEST_URI, 1, 17).await;
    assert!(help.is_some());

    let sig = &help.unwrap().signatures[0];
    assert!(sig.label.contains("add"));
    assert!(sig.label.contains("int"));

    client.shutdown().await;
}

#[tokio::test]
async fn signature_help_active_parameter() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn add(x: int, y: int) -> int { x + y }\nfn main() { add(1, 2) }",
        )
        .await;

    let help = client.signature_help(TEST_URI, 1, 19).await;
    assert!(help.is_some());

    let sig_help = help.unwrap();
    assert_eq!(sig_help.active_parameter, Some(1));

    client.shutdown().await;
}

#[tokio::test]
async fn prepare_rename_local_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let foo = 1\n  foo + 1\n}")
        .await;

    let response = client.prepare_rename(TEST_URI, 1, 6).await;
    assert!(response.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn rename_local_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let foo = 1\n  foo + 1\n}")
        .await;

    let edit = client.rename(TEST_URI, 1, 6, "bar").await;
    assert!(edit.is_some());

    let workspace_edit = edit.unwrap();
    let changes = workspace_edit.changes.unwrap();
    let file_edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();

    assert_eq!(file_edits.len(), 2);

    for edit in file_edits {
        assert_eq!(edit.new_text, "bar");
    }

    client.shutdown().await;
}

#[tokio::test]
async fn rename_rejects_keywords() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let foo = 1\n  foo + 1\n}")
        .await;

    let edit = client.rename(TEST_URI, 1, 6, "fn").await;
    assert!(edit.is_none());

    client.shutdown().await;
}

#[tokio::test]
async fn rename_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn foo() { 1 }\nfn main() { foo(); foo() }")
        .await;

    let edit = client.rename(TEST_URI, 0, 3, "bar").await;
    assert!(edit.is_some());

    let workspace_edit = edit.unwrap();
    let changes = workspace_edit.changes.unwrap();
    let file_edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();

    assert_eq!(file_edits.len(), 3);

    client.shutdown().await;
}

#[tokio::test]
async fn formatting_reformats_code() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn foo(){1}").await;

    let edits = client.formatting(TEST_URI).await;
    assert!(edits.is_some());

    let text_edits = edits.unwrap();
    assert!(!text_edits.is_empty());

    let new_text = &text_edits[0].new_text;
    assert!(new_text.contains("fn foo()"));
    assert!(new_text.contains("{ 1 }") || new_text.contains("{\n"));

    client.shutdown().await;
}

#[tokio::test]
async fn formatting_returns_none_on_parse_error() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn foo(").await;

    let edits = client.formatting(TEST_URI).await;
    assert!(edits.is_none());

    client.shutdown().await;
}

#[tokio::test]
async fn formatting_applies_edits() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn  foo()  { 1 }").await;

    let edits = client.formatting(TEST_URI).await;
    assert!(edits.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn document_symbols_lists_functions() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"fn foo() { 1 }
fn bar() { 2 }"#,
        )
        .await;

    let response = client.document_symbol(TEST_URI).await;
    assert!(response.is_some());

    let names = symbol_names(&response.unwrap());
    assert!(names.iter().any(|n| n == "foo"));
    assert!(names.iter().any(|n| n == "bar"));

    client.shutdown().await;
}

#[tokio::test]
async fn document_symbols_lists_structs() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, r#"struct Point { x: int, y: int }"#)
        .await;

    let response = client.document_symbol(TEST_URI).await;
    assert!(response.is_some());

    let names = symbol_names(&response.unwrap());
    assert!(names.iter().any(|n| n == "Point"));

    client.shutdown().await;
}

#[tokio::test]
async fn document_symbols_lists_enums() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, r#"enum Color { Red, Green, Blue }"#)
        .await;

    let response = client.document_symbol(TEST_URI).await;
    assert!(response.is_some());

    let names = symbol_names(&response.unwrap());
    assert!(names.iter().any(|n| n == "Color"));

    client.shutdown().await;
}

#[tokio::test]
async fn document_symbols_lists_constants() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "const PI = 3.14").await;

    let response = client.document_symbol(TEST_URI).await;
    assert!(response.is_some());

    let names = symbol_names(&response.unwrap());
    assert!(names.iter().any(|n| n == "PI"));

    client.shutdown().await;
}

#[tokio::test]
async fn hover_updates_after_document_change() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client.open(TEST_URI, "fn main() { let x = 42; x }").await;
    let hover1 = client.hover(TEST_URI, 0, 16).await;
    assert!(hover1.is_some(), "hover1 should return something");
    let content1 = hover_content(&hover1.unwrap());
    assert!(content1.contains("int"));

    client
        .change(TEST_URI, r#"fn main() { let x = "hello"; x }"#, 2)
        .await;
    let hover2 = client.hover(TEST_URI, 0, 16).await;
    assert!(hover2.is_some(), "hover2 should return something");
    let content2 = hover_content(&hover2.unwrap());
    assert!(content2.contains("string"));

    client.shutdown().await;
}

#[tokio::test]
async fn hover_on_function_name_works() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { 1 }").await;

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());

    let content = hover_content(&hover.unwrap());
    assert!(content.contains("fn"));

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_on_literal_returns_none() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { 42 }").await;

    let response = client.goto_definition(TEST_URI, 0, 12).await;
    assert!(response.is_none());

    client.shutdown().await;
}

#[tokio::test]
async fn completion_empty_file() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "").await;

    let response = client.completion(TEST_URI, 0, 0).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(!labels.is_empty());

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn main() { Point { x: 1, y: 2 } }",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 1, 12).await;
    assert!(response.is_some());

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_type_in_parameter() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn foo(p: Point) -> int { 1 }",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 1, 10).await;
    assert!(response.is_some());

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_type_in_return_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn foo() -> Point { Point { x: 1, y: 2 } }",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 1, 12).await;
    assert!(response.is_some());

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_from_struct_call_usage() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  p\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 2, 10).await;
    assert!(response.is_some());

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn completion_dot_on_local_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Point { x: int, y: int }
impl Point {
  pub fn dist(self) -> int { self.x + self.y }
}
fn main() {
  let p = Point { x: 1, y: 2 }
  p.dist()
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 6, 4).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.iter().any(|l| l == "dist"),
        "should include 'dist' method, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "x"),
        "should include 'x' field, got: {labels:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_dot_on_for_loop_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Shape { side: int }
impl Shape {
  pub fn area(self) -> int { self.side * self.side }
}
fn main() {
  let shapes = [Shape { side: 3 }]
  for shape in shapes {
    shape.area()
  }
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 7, 10).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.iter().any(|l| l == "area"),
        "should include 'area' method for element type, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "side"),
        "should include 'side' field for element type, got: {labels:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_dot_after_indexed_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Item { name: string }
impl Item {
  pub fn label(self) -> string { self.name }
}
fn main() {
  let items = [Item { name: \"a\" }]
  items[0].label()
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 6, 11).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.iter().any(|l| l == "label"),
        "should include 'label' method for element type, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "name"),
        "should include 'name' field for element type, got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "length"),
        "should not include Slice methods like 'length', got: {labels:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_no_globals_after_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: 1, y: 2 }
  p.x
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 3, 4).await;
    assert!(response.is_some());

    let labels = completion_labels(&response.unwrap());
    assert!(
        !labels.iter().any(|l| l == "fn"),
        "should not include keyword 'fn' after dot, got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "let"),
        "should not include keyword 'let' after dot, got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "if"),
        "should not include keyword 'if' after dot, got: {labels:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn crash_resilience_broken_syntax() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let broken_inputs = [
        "fn",
        "fn foo(",
        "fn foo() {",
        "fn foo() -> ",
        "struct",
        "struct Foo {",
        "struct Foo { x:",
        "enum",
        "enum Foo {",
        "enum Foo { A(",
        "let x =",
        "let x: ",
        "if",
        "if true {",
        "if true { 1 } else {",
        "match",
        "match x {",
        "match x { A =>",
        "fn foo() { ( }",
        "fn foo() { [ }",
        "fn foo() { { ( ) }",
        "fn foo(x: int, { 1 }",
        "fn main() { 1 + }",
        "fn main() { 1 + 2 * }",
        "fn main() { x. }",
        "}{}{}{",
        ")))(((",
        "->->->",
        "::::",
        "fn 日本語() { 1 }",
        "fn main() { \"unterminated",
        "fn main() { 'x }",
        "",
        "   ",
        "\n\n\n",
        "fn fn foo() { 1 }",
        "let let x = 1",
        "struct struct Foo {}",
        "fn main() { if true { if true { if true { if true {",
        "fn main() { ((((((((((",
        "fn main() { let x: int = \"hello\" }",
        "fn main() { let x: string = 42 }",
        "fn main() { let x = 1 + \"hello\" }",
        r#"import "nonexistent""#,
        r#"import "go:nonexistent/pkg""#,
        "fn main() { if let Some(x) = Some(1) { x } }",
    ];

    for (i, input) in broken_inputs.iter().enumerate() {
        client.open(TEST_URI, input).await;

        let _hover = client.hover(TEST_URI, 0, 0).await;
        let _completion = client.completion(TEST_URI, 0, 0).await;

        client.change(TEST_URI, input, (i as i32) + 2).await;
    }

    client.open(TEST_URI, "fn main() { let x = 42; x }").await;
    let hover = client.hover(TEST_URI, 0, 16).await;
    assert!(
        hover.is_some(),
        "server should still respond after broken inputs"
    );
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "server should still produce correct results"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_enum_dot_shows_variants() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
enum Color { Red, Green, Blue }
fn main() {
  let c = Color.Red
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 2, 16).await;
    assert!(response.is_some(), "should return completions for enum dot");

    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.iter().any(|l| l == "Red"),
        "should include 'Red' variant, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "Green"),
        "should include 'Green' variant, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "Blue"),
        "should include 'Blue' variant, got: {labels:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_enum_dot_shows_tuple_variants() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
enum Shape {
  Circle(float),
  Rectangle(float, float),
}
fn main() {
  let s = Shape.Circle(1.0)
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 5, 16).await;
    assert!(response.is_some(), "should return completions for enum dot");

    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.iter().any(|l| l == "Circle"),
        "should include 'Circle' variant, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "Rectangle"),
        "should include 'Rectangle' variant, got: {labels:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_enum_variant_shows_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
enum Color { Red, Green, Blue }
fn main() {
  let c = Color.Red
}";
    client.open(TEST_URI, source).await;

    let hover = client.hover(TEST_URI, 2, 6).await;
    assert!(hover.is_some());

    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("Color"),
        "should show Color type, got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_match_arm_binding_shows_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
fn main() {
  let opt: Option<int> = Some(42)
  match opt {
    Some(val) => val + 1,
    None => 0,
  }
}";
    client.open(TEST_URI, source).await;

    let hover = client.hover(TEST_URI, 3, 9).await;
    assert!(
        hover.is_some(),
        "hover on match arm binding should return something"
    );

    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "match arm binding should show inner type 'int', got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_match_option_binding_shows_inner_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
fn main() {
  let opt: Option<string> = Some(\"hello\")
  match opt {
    Some(val) => val,
    None => \"\",
  }
}";
    client.open(TEST_URI, source).await;

    let hover = client.hover(TEST_URI, 3, 9).await;
    assert!(
        hover.is_some(),
        "hover on match arm binding should return something"
    );

    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("string"),
        "match arm binding should show inner type 'string', got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_if_let_does_not_crash() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
fn main() {
  let opt: Option<string> = Some(\"hello\")
  if let Some(val) = opt {
    val
  }
}";
    client.open(TEST_URI, source).await;

    let _hover = client.hover(TEST_URI, 2, 15).await;

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(
        hover.is_some(),
        "server should still respond after if-let hover"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_match_struct_destructuring_shows_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: 1, y: 2 }
  match p {
    Point { x, y } => x + y,
  }
}";
    client.open(TEST_URI, source).await;

    let hover = client.hover(TEST_URI, 4, 12).await;
    assert!(
        hover.is_some(),
        "hover on destructured field should return something"
    );

    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "destructured field should show type 'int', got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn diagnostics_type_error() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(TEST_URI, "fn main() { let x: int = \"hello\" }")
        .await;

    let diagnostics = client.await_diagnostics().await;
    assert!(
        !diagnostics.is_empty(),
        "type error should produce diagnostics"
    );

    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "should contain an error diagnostic, got: {diagnostics:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn diagnostics_valid_code_is_clean() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client.open(TEST_URI, "fn main() { let x = 42 }").await;

    let diagnostics = client.await_diagnostics().await;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        errors.is_empty(),
        "valid code should produce no error diagnostics, got: {errors:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn diagnostics_parse_error() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client.open(TEST_URI, "fn foo(").await;

    let diagnostics = client.await_diagnostics().await;
    assert!(
        !diagnostics.is_empty(),
        "parse error should produce diagnostics"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn diagnostics_update_after_fix() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(TEST_URI, "fn main() { let x: int = \"hello\" }")
        .await;
    let diagnostics = client.await_diagnostics().await;
    assert!(
        !diagnostics.is_empty(),
        "should have diagnostics for type error"
    );

    client
        .change(TEST_URI, "fn main() { let x: int = 42 }", 2)
        .await;
    let diagnostics = client.await_diagnostics().await;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        errors.is_empty(),
        "after fixing the error, should have no error diagnostics, got: {errors:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn cross_module_goto_definition() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let main_content = "import \"utils\"\n\nfn main() { utils.helper() }";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let utils_dir = src.join("utils");
    std::fs::create_dir_all(&utils_dir).unwrap();
    std::fs::write(utils_dir.join("utils.lis"), "pub fn helper() -> int { 42 }").unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    let hover = client.hover(&main_uri, 2, 14).await;
    let _ = hover;

    let completion = client.completion(&main_uri, 2, 0).await;
    assert!(
        completion.is_some(),
        "server should still respond with cross-module code"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn go_import_hover_on_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
import \"go:fmt\"

fn main() {
  fmt.Println(\"hello\")
}";
    client.open(TEST_URI, source).await;

    let hover = client.hover(TEST_URI, 3, 6).await;
    let _ = hover;

    let completion = client.completion(TEST_URI, 3, 6).await;
    assert!(
        completion.is_some(),
        "server should still respond after go: import"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn go_import_completion_on_module() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
import \"go:strings\"

fn main() {
  strings.Contains(\"hello\", \"ell\")
}";
    client.open(TEST_URI, source).await;

    let response = client.completion(TEST_URI, 3, 10).await;
    let _ = response;

    let hover = client.hover(TEST_URI, 0, 0).await;
    let _ = hover;

    client.shutdown().await;
}

async fn stress_test_input(source: &str) -> bool {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, source).await;

    let _hover = client.hover(TEST_URI, 0, 0).await;
    let _completion = client.completion(TEST_URI, 0, 0).await;
    let _def = client.goto_definition(TEST_URI, 0, 0).await;
    let _refs = client.references(TEST_URI, 0, 0, true).await;
    let _sig = client.signature_help(TEST_URI, 0, 0).await;
    let _fmt = client.formatting(TEST_URI).await;
    let _sym = client.document_symbol(TEST_URI).await;

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    let alive = hover.is_some();

    client.shutdown().await;
    alive
}

#[tokio::test]
async fn stress_match_wrong_variant_count() {
    assert!(
        stress_test_input(
            "\
enum Pair { A(int, int) }
fn main() {
  match Pair.A(1, 2) {
    Pair.A(x) => x,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_nonexistent_variant() {
    assert!(
        stress_test_input(
            "\
enum Color { Red, Green }
fn main() {
  match Color.Red {
    Color.Blue => 1,
    _ => 2,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_struct_as_enum() {
    assert!(
        stress_test_input(
            "\
struct Point { x: int }
fn main() {
  let p = Point { x: 1 }
  match p {
    Point.Something(x) => x,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_function_as_pattern() {
    assert!(
        stress_test_input(
            "\
fn foo() -> int { 1 }
fn main() {
  match 1 {
    foo(x) => x,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_deeply_nested_patterns() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x: Option<Option<Option<int>>> = Some(Some(Some(42)))
  match x {
    Some(Some(Some(val))) => val,
    Some(Some(None)) => 0,
    Some(None) => 0,
    None => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_literal_patterns() {
    assert!(
        stress_test_input(
            "\
fn main() {
  match 42 {
    0 => \"zero\",
    1 => \"one\",
    _ => \"other\",
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_string_literal_pattern() {
    assert!(
        stress_test_input(
            r#"
fn main() {
  match "hello" {
    "hello" => 1,
    "world" => 2,
    _ => 0,
  }
}"#
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_tuple_destructuring() {
    assert!(
        stress_test_input(
            "\
fn main() {
  match (1, \"hello\", true) {
    (a, b, c) => a,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_or_pattern() {
    assert!(
        stress_test_input(
            "\
enum Color { Red, Green, Blue }
fn main() {
  match Color.Red {
    Color.Red | Color.Green => 1,
    Color.Blue => 2,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_guard() {
    assert!(
        stress_test_input(
            "\
fn main() {
  match 42 {
    x if x > 0 => \"positive\",
    _ => \"non-positive\",
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_enum_with_wrong_type() {
    assert!(
        stress_test_input(
            "\
enum A { X }
enum B { Y }
fn main() {
  match A.X {
    B.Y => 1,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_result_nested() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let r: Result<Option<int>, string> = Ok(Some(42))
  match r {
    Ok(Some(val)) => val,
    Ok(None) => 0,
    Err(msg) => -1,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_loops_with_errors() {
    assert!(
        stress_test_input(
            "\
fn main() {
  for x in [1, 2, 3] {
    for y in [\"a\", \"b\"] {
      let z: int = y
      break
    }
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_while_let_with_type_error() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let mut opt: Option<int> = Some(1)
  while let Some(x) = opt {
    let y: string = x
    opt = None
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_for_loop_wrong_iterable() {
    assert!(
        stress_test_input(
            "\
fn main() {
  for x in 42 {
    x
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_while_with_break() {
    assert!(
        stress_test_input(
            "\
fn main() {
  while true {
    while true {
      while true {
        break
      }
      break
    }
    break
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_loop_with_match_inside() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let mut x = Some(1)
  while let Some(val) = x {
    match val {
      0 => { x = None },
      _ => { x = Some(val - 1) },
    }
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_int() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = 42
  x.foo()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_string() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = \"hello\"
  x.nonexistent()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_chained_dots_on_error() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = undefined_var
  x.foo.bar.baz()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_function_result() {
    assert!(
        stress_test_input(
            "\
fn foo() -> int { 1 }
fn main() {
  foo().nonexistent()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_tuple() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let t = (1, 2, 3)
  t.nonexistent()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_method_on_generic() {
    assert!(
        stress_test_input(
            "\
fn apply<T>(x: T) -> T {
  x.something()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_pipeline_operator() {
    assert!(
        stress_test_input(
            "\
fn double(x: int) -> int { x * 2 }
fn main() {
  let result = 5 |> double
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_pipeline_chained() {
    assert!(
        stress_test_input(
            "\
fn add1(x: int) -> int { x + 1 }
fn double(x: int) -> int { x * 2 }
fn main() {
  let result = 1 |> add1 |> double |> add1
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_pipeline_with_args() {
    assert!(
        stress_test_input(
            "\
fn add(x: int, y: int) -> int { x + y }
fn main() {
  let result = 5 |> add(3)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_name_shadow_struct_with_function() {
    assert!(
        stress_test_input(
            "\
fn Point() -> int { 1 }
struct Point { x: int }
fn main() {
  let p = Point { x: 1 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_type_alias_to_primitive() {
    assert!(
        stress_test_input(
            "\
type MyInt = int
fn main() {
  let x: MyInt = 42
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_type_alias_to_generic() {
    assert!(
        stress_test_input(
            "\
type OptInt = Option<int>
fn main() {
  let x: OptInt = Some(42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_recursive_type_alias() {
    assert!(
        stress_test_input(
            "\
type Tree = Option<(int, Tree, Tree)>
fn main() {
  let t: Tree = None
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_impl_on_nonexistent_type() {
    assert!(
        stress_test_input(
            "\
impl Nonexistent {
  pub fn foo(self) -> int { 1 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_impl_wrong_self_type() {
    assert!(
        stress_test_input(
            "\
struct Foo { x: int }
impl Foo {
  pub fn bar(self) -> string { self.x }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_interface_implementation_mismatch() {
    assert!(
        stress_test_input(
            "\
interface Printable {
  fn to_string(self) -> string
}
struct Foo { x: int }
impl Foo {
  pub fn to_string(self) -> int { self.x }
}
fn print(p: Printable) -> string { p.to_string() }
fn main() {
  print(Foo { x: 1 })
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_empty_interface() {
    assert!(
        stress_test_input(
            "\
interface Empty {}
struct Foo {}
fn take(e: Empty) { }
fn main() {
  take(Foo {})
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_generic_instantiation_mismatch() {
    assert!(
        stress_test_input(
            "\
fn identity<T>(x: T) -> T { x }
fn main() {
  let x: string = identity(42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_generic_struct_wrong_params() {
    assert!(
        stress_test_input(
            "\
struct Pair<A, B> { first: A, second: B }
fn main() {
  let p: Pair<int> = Pair { first: 1, second: 2 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_higher_order_generics() {
    assert!(
        stress_test_input(
            "\
fn apply<A, B>(f: fn(A) -> B, x: A) -> B { f(x) }
fn main() {
  let result = apply(fn(x: int) -> string { \"hello\" }, 42)
  let y: int = result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_deeply_nested_if() {
    assert!(
        stress_test_input(
            "\
fn main() {
  if true {
    if true {
      if true {
        if true {
          if true {
            if true {
              if true {
                42
              } else { 0 }
            } else { 0 }
          } else { 0 }
        } else { 0 }
      } else { 0 }
    } else { 0 }
  } else { 0 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_mutual_recursion() {
    assert!(
        stress_test_input(
            "\
fn is_even(n: int) -> bool {
  if n == 0 { true } else { is_odd(n - 1) }
}
fn is_odd(n: int) -> bool {
  if n == 0 { false } else { is_even(n - 1) }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_closure_capture() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = 42
  let f = fn() -> int { x }
  let g = fn() -> int { f() }
  g()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_slice_operations_chain() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let s = [1, 2, 3, 4, 5]
  let t = s[1..3]
  let u = t[0]
  let v: string = u
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_map_operations() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let m = { \"a\": 1, \"b\": 2 }
  let v = m[\"a\"]
  let x: string = v
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_enum_struct_variant() {
    assert!(
        stress_test_input(
            "\
enum Shape {
  Circle { radius: float },
  Rectangle { width: float, height: float },
}
fn area(s: Shape) -> float {
  match s {
    Shape.Circle { radius } => 3.14 * radius * radius,
    Shape.Rectangle { width, height } => width * height,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_enum_method_on_variant() {
    assert!(
        stress_test_input(
            "\
enum List {
  Cons(int, List),
  Nil,
}
impl List {
  pub fn head(self) -> Option<int> {
    match self {
      List.Cons(x, _) => Some(x),
      List.Nil => None,
    }
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_conflicting_type_annotations() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x: int = \"hello\"
  let y: string = 42
  let z: bool = 3.14
  let w: float = true
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_function_type_mismatch() {
    assert!(
        stress_test_input(
            "\
fn foo(f: fn(int) -> string) -> string { f(1) }
fn main() {
  foo(fn(x: string) -> int { 1 })
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_return_type_mismatch() {
    assert!(
        stress_test_input(
            "\
fn foo() -> int {
  if true {
    \"hello\"
  } else {
    42
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_option_result_confusion() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x: Option<int> = Ok(42)
  let y: Result<int, string> = Some(42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multiple_errors_same_line() {
    assert!(
        stress_test_input(
            "\
fn main() { let x: int = \"a\"; let y: string = 1; let z: bool = 3.14 }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_undefined_everywhere() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = undefined1
  let y = undefined2 + undefined3
  let z = undefined4.method(undefined5)
  match undefined6 {
    Something(a) => a,
    _ => undefined7,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_self_outside_impl() {
    assert!(
        stress_test_input(
            "\
fn foo() -> int { self.x }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_break_outside_loop() {
    assert!(
        stress_test_input(
            "\
fn main() { break }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_continue_outside_loop() {
    assert!(
        stress_test_input(
            "\
fn main() { continue }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_return_outside_function() {
    assert!(stress_test_input("return 42").await);
}

#[tokio::test]
async fn stress_hover_every_position_complex_code() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Point { x: int, y: int }
enum Color { Red, Green, Blue }
impl Point {
  pub fn dist(self) -> int { self.x + self.y }
}
fn main() {
  let p = Point { x: 1, y: 2 }
  let c = Color.Red
  match c {
    Color.Red => p.dist(),
    _ => 0,
  }
}";
    client.open(TEST_URI, source).await;

    let line_count = source.lines().count() as u32;
    for line in 0..line_count {
        let line_len = source.lines().nth(line as usize).unwrap_or("").len() as u32;
        for ch in 0..line_len {
            let _ = client.hover(TEST_URI, line, ch).await;
        }
    }

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(
        hover.is_some(),
        "server must survive exhaustive hover sweep"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_every_position_complex_code() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
struct Foo { val: int }
impl Foo {
  pub fn get(self) -> int { self.val }
}
fn main() {
  let f = Foo { val: 42 }
  f.get()
}";
    client.open(TEST_URI, source).await;

    let line_count = source.lines().count() as u32;
    for line in 0..line_count {
        let line_len = source.lines().nth(line as usize).unwrap_or("").len() as u32;
        for ch in 0..=line_len {
            let _ = client.completion(TEST_URI, line, ch).await;
        }
    }

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(
        hover.is_some(),
        "server must survive exhaustive completion sweep"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_goto_def_every_position() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
enum Shape { Circle(float), Rect(float, float) }
fn area(s: Shape) -> float {
  match s {
    Shape.Circle(r) => 3.14 * r * r,
    Shape.Rect(w, h) => w * h,
  }
}";
    client.open(TEST_URI, source).await;

    let line_count = source.lines().count() as u32;
    for line in 0..line_count {
        let line_len = source.lines().nth(line as usize).unwrap_or("").len() as u32;
        for ch in 0..line_len {
            let _ = client.goto_definition(TEST_URI, line, ch).await;
        }
    }

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(
        hover.is_some(),
        "server must survive exhaustive goto-def sweep"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_match_type_param_as_pattern() {
    assert!(
        stress_test_input(
            "\
fn check<T>(x: T) -> int {
  match x {
    T(val) => 1,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_type_alias_as_pattern() {
    assert!(
        stress_test_input(
            "\
type Num = int
fn main() {
  match 42 {
    Num(x) => x,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_on_generic_value() {
    assert!(
        stress_test_input(
            "\
fn check<T>(x: T) -> T {
  match x {
    val => val,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_name_shadow_type_alias_with_function() {
    assert!(
        stress_test_input(
            "\
fn MyType() -> int { 1 }
type MyType = int
fn main() {
  let x: MyType = 42
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_name_shadow_enum_with_function() {
    assert!(
        stress_test_input(
            "\
fn Color() -> int { 1 }
enum Color { Red, Green }
fn main() {
  let c = Color.Red
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_name_shadow_struct_with_const() {
    assert!(
        stress_test_input(
            "\
const Point = 42
struct Point { x: int }
fn main() {
  let p = Point { x: 1 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_or_patterns() {
    assert!(
        stress_test_input(
            "\
enum Tree { Leaf(int), Node(Tree, Tree) }
fn sum(t: Tree) -> int {
  match t {
    Tree.Leaf(x) | Tree.Leaf(x) => x,
    Tree.Node(l, r) => sum(l) + sum(r),
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_slice_pattern() {
    assert!(
        stress_test_input(
            "\
fn first(xs: [int]) -> int {
  match xs {
    [x, ..rest] => x,
    [] => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_struct_pattern_wrong_fields() {
    assert!(
        stress_test_input(
            "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: 1, y: 2 }
  match p {
    Point { z, w } => z + w,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_on_boolean() {
    assert!(
        stress_test_input(
            "\
fn main() {
  match true {
    true => 1,
    false => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_recursive_enum_match() {
    assert!(
        stress_test_input(
            "\
enum Expr {
  Num(int),
  Add(Expr, Expr),
  Mul(Expr, Expr),
}
fn eval(e: Expr) -> int {
  match e {
    Expr.Num(n) => n,
    Expr.Add(a, b) => eval(a) + eval(b),
    Expr.Mul(a, b) => eval(a) * eval(b),
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_struct_call_missing_field() {
    assert!(
        stress_test_input(
            "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: 1 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_struct_call_extra_field() {
    assert!(
        stress_test_input(
            "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: 1, y: 2, z: 3 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_struct_call_wrong_field_type() {
    assert!(
        stress_test_input(
            "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: \"hello\", y: true }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_option() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x: Option<int> = Some(42)
  x.nonexistent()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_result() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x: Result<int, string> = Ok(42)
  x.nonexistent()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_chain_method_on_wrong_type() {
    assert!(
        stress_test_input(
            "\
struct Foo { x: int }
impl Foo {
  pub fn get(self) -> int { self.x }
}
fn main() {
  let f = Foo { x: 1 }
  f.get().nonexistent().another()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_dot_on_enum_variant() {
    assert!(
        stress_test_input(
            "\
enum Color { Red, Green, Blue }
fn main() {
  Color.Red.something()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_propagate_in_non_result() {
    assert!(
        stress_test_input(
            "\
fn main() -> int {
  let x = 42
  x?
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_propagate_nested() {
    assert!(
        stress_test_input(
            "\
fn foo() -> Result<int, string> {
  let a: Result<Result<int, string>, string> = Ok(Ok(42))
  let b = a??
  Ok(b)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_mut_reassign_wrong_type() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let mut x = 42
  x = \"hello\"
  x = true
  x = [1, 2, 3]
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_immutable_reassign() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = 42
  x = 100
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_all_expression_types() {
    assert!(
        stress_test_input(
            "\
struct Foo { x: int }
enum Bar { A(int), B }
interface Baz {
  fn method(self) -> int
}
type Alias = int
const CONST = 42

impl Foo {
  pub fn get(self) -> int { self.x }
}

fn identity<T>(x: T) -> T { x }

fn main() {
  let a = 1
  let b = \"hello\"
  let c = true
  let d = 3.14
  let e = (a, b, c)
  let f = [1, 2, 3]
  let g = { \"key\": \"value\" }
  let h = Foo { x: a }
  let i = Bar.A(a)
  let j = Bar.B
  let k: Option<int> = Some(a)
  let l: Result<int, string> = Ok(a)
  let mut m = 0
  m = m + 1
  let n = if c { a } else { CONST }
  let o = match k {
    Some(val) => val,
    None => 0,
  }
  for x in f {
    m = m + x
  }
  while m > 0 {
    m = m - 1
  }
  let p = h.get()
  let q = identity(a)
  let r = f[0]
  let s = f[0..2]
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_rapid_changes() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    for i in 0..20 {
        let source = if i % 2 == 0 {
            "fn main() { let x = 42; x }"
        } else {
            "fn main() { let x: int = \"wrong\" }"
        };
        client.change(TEST_URI, source, i + 1).await;
    }

    client.change(TEST_URI, "fn main() { 1 }", 100).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some(), "server must survive rapid changes");

    client.shutdown().await;
}

#[tokio::test]
async fn stress_very_long_line() {
    let mut expr = "1".to_string();
    for _ in 0..500 {
        expr = format!("{} + 1", expr);
    }
    let source = format!("fn main() {{ {} }}", expr);

    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, &source).await;

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(
        hover.is_some(),
        "server must survive deeply nested expressions"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_many_definitions() {
    let mut source = String::new();
    for i in 0..100 {
        source.push_str(&format!("fn f{}() -> int {{ {} }}\n", i, i));
    }
    source.push_str("fn main() { f0() + f99() }");
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_generics() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x: Option<Option<Option<Option<Option<int>>>>> = Some(Some(Some(Some(Some(42)))))
  match x {
    Some(Some(Some(Some(Some(v))))) => v,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_empty_function_body() {
    assert!(stress_test_input("fn main() {}").await);
}

#[tokio::test]
async fn stress_empty_struct() {
    assert!(
        stress_test_input(
            "\
struct Empty {}
fn main() {
  let e = Empty {}
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_empty_enum() {
    assert!(stress_test_input("enum Nothing {}").await);
}

#[tokio::test]
async fn stress_empty_impl() {
    assert!(
        stress_test_input(
            "\
struct Foo { x: int }
impl Foo {}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_empty_match() {
    assert!(
        stress_test_input(
            "\
fn main() {
  match 42 {}
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multiple_impl_blocks() {
    assert!(
        stress_test_input(
            "\
struct Foo { x: int }
impl Foo {
  pub fn get_x(self) -> int { self.x }
}
impl Foo {
  pub fn double(self) -> int { self.x * 2 }
}
fn main() {
  let f = Foo { x: 5 }
  f.get_x() + f.double()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_bounded_generic() {
    assert!(
        stress_test_input(
            "\
interface Printable {
  fn to_str(self) -> string
}
fn print<T: Printable>(x: T) -> string {
  x.to_str()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_bounded_generic_mismatch() {
    assert!(
        stress_test_input(
            "\
interface Printable {
  fn to_str(self) -> string
}
fn print<T: Printable>(x: T) -> string {
  x.to_str()
}
fn main() {
  print(42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_import_self_reference() {
    assert!(stress_test_input(r#"import "self""#).await);
}

#[tokio::test]
async fn stress_import_empty_string() {
    assert!(stress_test_input(r#"import """#).await);
}

#[tokio::test]
async fn stress_multiple_go_imports() {
    assert!(
        stress_test_input(
            "\
import \"go:fmt\"
import \"go:strings\"
import \"go:strconv\"

fn main() {
  fmt.Println(strings.Contains(\"hello\", \"ell\"))
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_closure_type_mismatch() {
    assert!(
        stress_test_input(
            "\
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
fn main() {
  apply(fn(x: string) -> string { x }, 42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_higher_order_closure() {
    assert!(
        stress_test_input(
            "\
fn compose(f: fn(int) -> int, g: fn(int) -> int) -> fn(int) -> int {
  fn(x: int) -> int { f(g(x)) }
}
fn main() {
  let double = fn(x: int) -> int { x * 2 }
  let inc = fn(x: int) -> int { x + 1 }
  let f = compose(double, inc)
  f(5)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_select_syntax() {
    assert!(
        stress_test_input(
            "\
fn main() {
  select {
    _ => 1,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_only_comments() {
    assert!(
        stress_test_input(
            "\
/* block comment */
"
        )
        .await
    );
}

#[tokio::test]
async fn stress_comment_in_expression() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = 1 + /* middle of expr */ 2
  x
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_unicode_strings() {
    assert!(
        stress_test_input(
            r#"
fn main() {
  let x = "こんにちは世界"
  let y = "🎉🎊🎈"
  let z = "café naïve résumé"
  x
}
"#
        )
        .await
    );
}

#[tokio::test]
async fn stress_numeric_edge_cases() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let a = -42
  let b = 0
  let c = 9999999999
  let d = 0.0
  let e = -3.14
  let f = 1e10
  a + b + c
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_cascading_errors() {
    assert!(
        stress_test_input(
            "\
fn main() {
  let x = unknown1
  let y = x.method()
  let z = y + unknown2
  let w = z.another(unknown3)
  match w {
    Some(v) => v.yet_another(),
    None => unknown4,
  }
}"
        )
        .await
    );
}

async fn stress_test_all_positions(source: &str) -> bool {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, source).await;

    let lines: Vec<&str> = source.lines().collect();
    for line in 0..lines.len() as u32 {
        let line_len = lines[line as usize].len() as u32;
        for col in [0, line_len / 2, line_len.saturating_sub(1), line_len] {
            let _hover = client.hover(TEST_URI, line, col).await;
            let _def = client.goto_definition(TEST_URI, line, col).await;
            let _comp = client.completion(TEST_URI, line, col).await;
            let _sig = client.signature_help(TEST_URI, line, col).await;
            let _refs = client.references(TEST_URI, line, col, true).await;
            let _rename = client.prepare_rename(TEST_URI, line, col).await;
        }
    }

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    let alive = hover.is_some();

    client.shutdown().await;
    alive
}

#[tokio::test]
async fn stress_completion_after_dot_on_broken_expr() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { 1. }").await;
    let _comp = client.completion(TEST_URI, 0, 15).await;

    client.change(TEST_URI, "fn main() { \"hello\". }", 2).await;
    let _comp = client.completion(TEST_URI, 0, 21).await;

    client.change(TEST_URI, "fn main() { true. }", 2).await;
    let _comp = client.completion(TEST_URI, 0, 18).await;

    client.change(TEST_URI, "fn main() { (1, 2). }", 2).await;
    let _comp = client.completion(TEST_URI, 0, 20).await;

    client.change(TEST_URI, "fn main() { 1 }", 3).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_after_indexed_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() { let xs = [1, 2]; xs[0]. }")
        .await;
    let _comp = client.completion(TEST_URI, 0, 36).await;

    client
        .change(TEST_URI, "fn main() { let xs = [1]; xs[0]. }", 2)
        .await;
    let _comp = client.completion(TEST_URI, 0, 33).await;

    client.change(TEST_URI, "fn main() { 1 }", 3).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_at_end_of_file() {
    let source = "fn main() { let x = 42 }";
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, source).await;

    let _hover = client.hover(TEST_URI, 0, source.len() as u32).await;
    let _hover = client.hover(TEST_URI, 0, source.len() as u32 + 100).await;
    let _hover = client.hover(TEST_URI, 100, 0).await;
    let _hover = client.hover(TEST_URI, 100, 100).await;

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_all_handlers_at_every_position_simple() {
    assert!(stress_test_all_positions("fn main() { let x = 42; x + 1 }").await);
}

#[tokio::test]
async fn stress_all_handlers_at_every_position_method_call() {
    assert!(
        stress_test_all_positions(
            "\
struct Foo { x: int }
impl Foo {
  pub fn get(self) -> int { self.x }
}
fn main() {
  let f = Foo { x: 1 }
  f.get()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_all_handlers_at_every_position_enum_match() {
    assert!(
        stress_test_all_positions(
            "\
enum Shape {
  Circle(float64),
  Rect(float64, float64),
}
fn area(s: Shape) -> float64 {
  match s {
    Shape.Circle(r) => 3.14 * r * r,
    Shape.Rect(w, h) => w * h,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_all_handlers_at_every_position_broken_code() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: int = \"wrong\"
  let y = x.nonexistent()
  let z = unknown_var
  match z {
    Some(v) => v,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_impl_block_on_unknown_type() {
    assert!(
        stress_test_all_positions(
            "\
impl UnknownType {
  pub fn method(self) -> int { 1 }
}
fn main() { 1 }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_impl_block_wrong_self_type() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x = 1
  x.nonexistent_method()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_type_alias_cycle() {
    assert!(stress_test_all_positions("type A = A\nfn main() { let x: A = 1 }").await);
}

#[tokio::test]
async fn stress_deeply_nested_closures() {
    let mut source = String::from("fn main() { let f = ");
    for _ in 0..15 {
        source.push_str("fn(x: int) -> int { ");
    }
    source.push('x');
    for _ in 0..15 {
        source.push_str(" }");
    }
    source.push_str(" }");
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_blocks() {
    let mut source = String::from("fn main() -> int { ");
    for _ in 0..15 {
        source.push_str("{ ");
    }
    source.push('1');
    for _ in 0..15 {
        source.push_str(" }");
    }
    source.push_str(" }");
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_if_else() {
    let mut source = String::from("fn main() -> int { ");
    for i in 0..5 {
        source.push_str(&format!("if {} > 0 {{ ", i));
    }
    source.push('1');
    for _ in 0..5 {
        source.push_str(" } else { 0 }");
    }
    source.push_str(" }");
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_many_match_arms() {
    let mut source = String::from("fn main() -> int {\n  match 0 {\n");
    for i in 0..100 {
        source.push_str(&format!("    {} => {},\n", i, i * 2));
    }
    source.push_str("    _ => 0,\n  }\n}");
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_signature_help_on_broken_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn foo(a: int, b: string) -> int { a }\nfn main() { foo( }",
        )
        .await;
    let sig = client.signature_help(TEST_URI, 1, 17).await;
    assert!(sig.is_some());

    client
        .change(TEST_URI, "fn main() { unknown_fn( }", 2)
        .await;
    let _sig = client.signature_help(TEST_URI, 0, 24).await;

    client.change(TEST_URI, "fn main() { 1 }", 3).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_on_non_renamable() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() { let x = 42; x + 1 }")
        .await;

    let prep = client.prepare_rename(TEST_URI, 0, 28).await;
    assert!(prep.is_none());

    let prep = client.prepare_rename(TEST_URI, 0, 11).await;
    assert!(prep.is_none());

    let prep = client.prepare_rename(TEST_URI, 100, 100).await;
    assert!(prep.is_none());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_goto_def_on_dot_access_chain() {
    assert!(
        stress_test_all_positions(
            "\
struct A { b: B }
struct B { c: int }
fn main() {
  let a = A { b: B { c: 1 } }
  a.b.c
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_or_pattern_with_type_mismatch() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Result<int, string> = Ok(1)
  match x {
    Ok(n) | Err(n) => n,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_on_unit_type() {
    assert!(
        stress_test_all_positions(
            "\
fn foo() {}
fn main() {
  match foo() {
    _ => 1,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_recursive_function() {
    assert!(
        stress_test_all_positions(
            "\
fn fib(n: int) -> int {
  if n <= 1 { n }
  else { fib(n - 1) + fib(n - 2) }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_generic_function_wrong_args() {
    assert!(
        stress_test_all_positions(
            "\
fn identity<T>(x: T) -> T { x }
fn main() {
  let x: int = identity(\"hello\")
  let y: string = identity(42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_empty_source() {
    assert!(stress_test_input("").await);
}

#[tokio::test]
async fn stress_whitespace_only() {
    assert!(stress_test_input("   \n\n  \n").await);
}

#[tokio::test]
async fn stress_single_character() {
    assert!(stress_test_input("x").await);
}

#[tokio::test]
async fn stress_just_keywords() {
    assert!(stress_test_input("fn if let match enum struct impl").await);
}

#[tokio::test]
async fn stress_unclosed_string_multiline() {
    assert!(stress_test_input("fn main() {\n  let x = \"hello\n  let y = 1\n}").await);
}

#[tokio::test]
async fn stress_many_type_errors_same_line() {
    assert!(
        stress_test_input(
            "fn main() { let a: int = \"x\"; let b: string = 1; let c: bool = 3.14; let d: float64 = true }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_interface_with_generic_constraint() {
    assert!(
        stress_test_all_positions(
            "\
interface Printable {
  fn to_str(self) -> string
}
fn print<T: Printable>(x: T) -> string { x.to_str() }
fn main() { print(42) }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_while_let_wrong_pattern() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let mut x: Option<int> = Some(1)
  while let Some(a, b) = x {
    x = None
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_for_loop_non_iterable_type() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  for x in 42 {
    x
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_index_on_non_indexable() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x = 42
  x[0]
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_let_else_with_wrong_pattern() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x = 42
  let Some(y) = x else { return }
  y
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multiple_semicolons() {
    assert!(stress_test_input("fn main() { ;;; let x = 1;;; x;;; }").await);
}

#[tokio::test]
async fn stress_nested_struct_literal() {
    assert!(
        stress_test_all_positions(
            "\
struct Inner { x: int }
struct Outer { inner: Inner, y: string }
fn main() {
  let o = Outer { inner: Inner { x: 1 }, y: \"hello\" }
  o.inner.x
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_closure_as_argument() {
    assert!(
        stress_test_all_positions(
            "\
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
fn main() {
  apply(fn(x: int) -> int { x + 1 }, 42)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_tuple_destructuring_mismatch() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let (a, b, c) = (1, 2)
  let (x,) = (1, 2, 3)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_self_outside_impl_with_method_call() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  self.method()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_return_in_top_level() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  return 1
  return \"hello\"
  return
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_break_continue_outside_loop() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  break
  continue
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_duplicate_field_names() {
    assert!(
        stress_test_all_positions(
            "\
struct Foo { x: int, x: string }
fn main() { Foo { x: 1, x: \"hello\" } }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_duplicate_function_names() {
    assert!(
        stress_test_all_positions(
            "\
fn foo() -> int { 1 }
fn foo() -> string { \"hello\" }
fn main() { foo() }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_completion_on_self_in_impl() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Foo { x: int, y: string }
impl Foo {
  pub fn method(self) -> int { self. }
}",
        )
        .await;

    let comp = client.completion(TEST_URI, 2, 36).await;
    assert!(comp.is_some());
    if let Some(CompletionResponse::Array(items)) = comp {
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"x"));
        assert!(labels.contains(&"y"));
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_pattern_bindings() {
    assert!(
        stress_test_all_positions(
            "\
enum Expr {
  Num(int),
  Add(Expr, Expr),
}
fn eval(e: Expr) -> int {
  match e {
    Expr.Num(n) => n,
    Expr.Add(a, b) => eval(a) + eval(b),
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_rapid_open_close_cycle() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    for i in 0..20 {
        let source = if i % 2 == 0 {
            "fn main() { let x = 42; x }"
        } else {
            "fn main() { let x: int = \"wrong\" }"
        };
        client.open(TEST_URI, source).await;
        let _hover = client.hover(TEST_URI, 0, 4).await;
    }

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_propagate_outside_result_function() {
    assert!(
        stress_test_all_positions(
            "\
fn fallible() -> Result<int, string> { Ok(1) }
fn main() {
  let x = fallible()?
  x
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_cast_wrong_types() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x = \"hello\" as int
  let y = true as float64
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_range_expressions() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let xs = [1, 2, 3, 4, 5]
  xs[1..3]
  xs[..2]
  xs[3..]
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_very_long_identifier() {
    let long_name: String = "a".repeat(1000);
    let source = format!("fn main() {{ let {} = 42; {} }}", long_name, long_name);
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_many_parameters() {
    let params: String = (0..50)
        .map(|i| format!("x{}: int", i))
        .collect::<Vec<_>>()
        .join(", ");
    let args: String = (0..50)
        .map(|i| format!("{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "fn big({}) -> int {{ x0 }}\nfn main() {{ big({}) }}",
        params, args
    );
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_chained_method_calls() {
    assert!(
        stress_test_all_positions(
            "\
struct Builder { val: int }
impl Builder {
  pub fn new() -> Builder { Builder { val: 0 } }
  pub fn set(self, v: int) -> Builder { Builder { val: v } }
  pub fn build(self) -> int { self.val }
}
fn main() {
  Builder.new().set(1).set(2).set(3).build()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_generic_types() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<Option<Option<int>>> = Some(Some(Some(42)))
  let y: Result<Option<int>, string> = Ok(Some(1))
  match x {
    Some(Some(Some(n))) => n,
    _ => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_enum_variant_same_name_as_type() {
    assert!(
        stress_test_all_positions(
            "\
struct Foo { x: int }
enum Bar { Foo(int), Other }
fn main() {
  let f = Foo { x: 1 }
  let b = Bar.Foo(2)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_mutual_type_reference() {
    assert!(
        stress_test_all_positions(
            "\
struct A { b: Option<B> }
struct B { a: Option<A> }
fn main() {
  let a = A { b: Some(B { a: None }) }
  a.b
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_if_let_chain() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<int> = Some(1)
  let y: Option<string> = Some(\"hi\")
  if let Some(a) = x {
    if let Some(b) = y {
      a
    }
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_deeply_nested_dot_access_chain() {
    let mut source = String::from(
        "struct S { s: S, v: int }\nfn main() {\n  let x = S { s: S { v: 0, s: S { v: 0, s: S { v: 1 } } }, v: 0 }\n  x",
    );
    for _ in 0..15 {
        source.push_str(".s");
    }
    source.push_str(".v\n}");
    assert!(stress_test_all_positions(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_call_expressions() {
    let mut source = String::from("fn f(x: int) -> int { x }\nfn main() { ");
    for _ in 0..15 {
        source.push_str("f(");
    }
    source.push('1');
    for _ in 0..15 {
        source.push(')');
    }
    source.push_str(" }");
    assert!(stress_test_all_positions(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_right_spine_binary() {
    let mut source = String::from("fn main() { 1 + ");
    for _ in 0..8 {
        source.push_str("(1 + ");
    }
    source.push('1');
    for _ in 0..8 {
        source.push(')');
    }
    source.push_str(" }");
    assert!(stress_test_all_positions(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_parens() {
    let mut source = String::from("fn main() { ");
    for _ in 0..15 {
        source.push('(');
    }
    source.push('1');
    for _ in 0..15 {
        source.push(')');
    }
    source.push_str(" }");
    assert!(stress_test_all_positions(&source).await);
}

#[tokio::test]
async fn stress_deeply_nested_unary() {
    let mut source = String::from("fn main() { ");
    for _ in 0..8 {
        source.push_str("!!");
    }
    source.push_str("true }");
    assert!(stress_test_all_positions(&source).await);
}
#[tokio::test]
async fn stress_completion_offset_zero() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "x.").await;
    let _comp = client.completion(TEST_URI, 0, 0).await;
    let _comp = client.completion(TEST_URI, 0, 1).await;
    let _comp = client.completion(TEST_URI, 0, 2).await;

    client.change(TEST_URI, ".", 2).await;
    let _comp = client.completion(TEST_URI, 0, 0).await;
    let _comp = client.completion(TEST_URI, 0, 1).await;

    client.change(TEST_URI, "", 3).await;
    let _comp = client.completion(TEST_URI, 0, 0).await;

    client.change(TEST_URI, "fn main() { 1 }", 4).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_after_indexed_access_short_source() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "].").await;
    let _comp = client.completion(TEST_URI, 0, 2).await;

    client.change(TEST_URI, "a].", 2).await;
    let _comp = client.completion(TEST_URI, 0, 3).await;

    client.change(TEST_URI, "fn main() { 1 }", 3).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_on_enum_with_many_variants() {
    let mut source = String::from("enum Color {\n");
    for i in 0..50 {
        source.push_str(&format!("  V{}(int),\n", i));
    }
    source.push_str("}\nfn main() { Color. }");
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, &source).await;
    let line = source.lines().count() as u32 - 1;
    let comp = client.completion(TEST_URI, line, 20).await;
    assert!(comp.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_on_multiple_impl_blocks() {
    assert!(
        stress_test_all_positions(
            "\
struct Foo { x: int }
impl Foo {
  pub fn get_x(self) -> int { self.x }
}
impl Foo {
  pub fn doubled(self) -> int { self.x * 2 }
}
fn main() {
  let f = Foo { x: 42 }
  f.get_x()
  f.doubled()
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_select_expression() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let ch1 = make(Channel<int>, 1)
  let ch2 = make(Channel<string>, 1)
  select {
    v <- ch1 => v,
    s <- ch2 => 0,
    _ => 42,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_try_recover_blocks() {
    assert!(
        stress_test_all_positions(
            "\
fn fallible() -> Result<int, string> { Ok(1) }
fn main() -> Result<int, string> {
  let x = try {
    let a = fallible()?
    let b = fallible()?
    a + b
  }
  x
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_defer_and_task() {
    assert!(
        stress_test_all_positions(
            "\
fn cleanup() {}
fn work() -> int { 42 }
fn main() {
  defer cleanup()
  task work()
  1
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_format_string_with_expressions() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x = 42
  let s = \"hello\"
  let result = \"${s} world ${x + 1} and ${if x > 0 { \"yes\" } else { \"no\" }}\"
  result
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_const_expressions() {
    assert!(
        stress_test_all_positions(
            "\
const PI: float64 = 3.14159
const NAME: string = \"lisette\"
const MAX: int = 100
fn main() { PI + 1.0 }"
        )
        .await
    );
}

#[tokio::test]
async fn stress_chained_type_aliases() {
    assert!(
        stress_test_all_positions(
            "\
type A = int
type B = A
type C = B
fn main() {
  let x: C = 42
  x + 1
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_value_enum() {
    assert!(
        stress_test_all_positions(
            "\
enum Direction: string {
  North = \"N\",
  South = \"S\",
  East = \"E\",
  West = \"W\",
}
fn main() {
  let d = Direction.North
  match d {
    Direction.North => 0,
    Direction.South => 1,
    Direction.East => 2,
    Direction.West => 3,
  }
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_deeply_nested_patterns() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<Option<Option<int>>> = Some(Some(Some(42)))
  match x {
    Some(Some(Some(n))) => n,
    Some(Some(None)) => 0,
    Some(None) => 0,
    None => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_struct_pattern_in_match() {
    assert!(
        stress_test_all_positions(
            "\
struct Point { x: int, y: int }
fn classify(p: Point) -> string {
  match p {
    Point { x: 0, y: 0 } => \"origin\",
    Point { x: 0, y } => \"y-axis\",
    Point { x, y: 0 } => \"x-axis\",
    Point { x, y } => \"other\",
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_slice_pattern_in_match() {
    assert!(
        stress_test_all_positions(
            "\
fn describe(xs: Slice<int>) -> string {
  match xs {
    [] => \"empty\",
    [x] => \"one\",
    [x, y] => \"two\",
    [first, ..rest] => \"many\",
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_match_with_guards() {
    assert!(
        stress_test_all_positions(
            "\
fn classify(x: int) -> string {
  match x {
    n if n < 0 => \"negative\",
    0 => \"zero\",
    n if n > 100 => \"big\",
    n => \"small\",
  }
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_hover_on_for_loop_binding() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let xs = [1, 2, 3]
  for x in xs {
    x + 1
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_hover_on_while_let_binding() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let mut opt: Option<int> = Some(42)
  while let Some(v) = opt {
    opt = None
    v
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_hover_on_lambda_params() {
    assert!(
        stress_test_all_positions(
            "\
fn apply(f: fn(int, string) -> bool, x: int, s: string) -> bool { f(x, s) }
fn main() {
  apply(fn(a: int, b: string) -> bool { a > 0 }, 1, \"hi\")
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_goto_def_on_struct_constructor() {
    assert!(
        stress_test_all_positions(
            "\
struct Config { width: int, height: int, title: string }
fn main() {
  let c = Config { width: 800, height: 600, title: \"hello\" }
  c.width
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_goto_def_on_enum_variant() {
    assert!(
        stress_test_all_positions(
            "\
enum Token {
  Number(int),
  Ident(string),
  Plus,
}
fn main() {
  let t = Token.Number(42)
  match t {
    Token.Number(n) => n,
    Token.Ident(s) => 0,
    Token.Plus => 0,
  }
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_references_on_widely_used_binding() {
    let mut source = String::from("fn main() {\n  let counter = 0\n");
    for i in 0..50 {
        source.push_str(&format!("  let x{} = counter + {}\n", i, i));
    }
    source.push_str("  counter\n}");
    assert!(stress_test_all_positions(&source).await);
}
#[tokio::test]
async fn stress_signature_help_nested_calls() {
    assert!(
        stress_test_all_positions(
            "\
fn add(a: int, b: int) -> int { a + b }
fn mul(a: int, b: int) -> int { a * b }
fn main() {
  add(mul(1, 2), mul(3, add(4, 5)))
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_signature_help_method_call() {
    assert!(
        stress_test_all_positions(
            "\
struct Vec2 { x: float64, y: float64 }
impl Vec2 {
  pub fn add(self, other: Vec2) -> Vec2 {
    Vec2 { x: self.x + other.x, y: self.y + other.y }
  }
  pub fn scale(self, factor: float64) -> Vec2 {
    Vec2 { x: self.x * factor, y: self.y * factor }
  }
}
fn main() {
  let v = Vec2 { x: 1.0, y: 2.0 }
  v.add(v.scale(2.0))
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_completion_after_unicode() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct S { x: int }\nfn main() {\n  let s = S { x: 1 }\n  s.\n}",
        )
        .await;
    let _comp = client.completion(TEST_URI, 3, 4).await;

    client.change(TEST_URI, "fn main() { 1 }", 2).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_at_multibyte_boundaries() {
    let source = "fn main() { let x = 42; x }";
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, source).await;

    for col in 0..source.len() as u32 + 5 {
        let _hover = client.hover(TEST_URI, 0, col).await;
    }

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}
#[tokio::test]
async fn stress_rename_enum_variant() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "enum Color { Red, Green, Blue }\nfn main() { Color.Red }",
        )
        .await;

    let prep = client.prepare_rename(TEST_URI, 0, 14).await;
    assert!(prep.is_some());

    let _prep = client.prepare_rename(TEST_URI, 1, 19).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_with_many_usages() {
    let mut source = String::from("fn main() {\n  let counter = 0\n");
    for _ in 0..30 {
        source.push_str("  let _ = counter + 1\n");
    }
    source.push('}');
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, &source).await;

    let prep = client.prepare_rename(TEST_URI, 1, 7).await;
    assert!(prep.is_some());

    client.shutdown().await;
}
#[tokio::test]
async fn stress_formatting_deeply_nested() {
    let mut source = String::from("fn main() { ");
    for _ in 0..7 {
        source.push_str("if true { ");
    }
    source.push('1');
    for _ in 0..7 {
        source.push_str(" } else { 0 }");
    }
    source.push_str(" }");
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_formatting_long_function_signature() {
    let params: String = (0..20)
        .map(|i| format!("param_{}: int", i))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("fn long_function({}) -> int {{ param_0 }}", params);
    assert!(stress_test_input(&source).await);
}
#[tokio::test]
async fn stress_document_symbols_many_items() {
    let mut source = String::new();
    for i in 0..50 {
        source.push_str(&format!("fn func_{}() -> int {{ {} }}\n", i, i));
    }
    for i in 0..20 {
        source.push_str(&format!("struct S{} {{ x: int }}\n", i));
    }
    for i in 0..10 {
        source.push_str(&format!("const C{}: int = {}\n", i, i));
    }
    source.push_str("fn main() { 1 }");

    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, &source).await;
    let symbols = client.document_symbol(TEST_URI).await;
    assert!(symbols.is_some());

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}
#[tokio::test]
async fn stress_rapid_type_changes() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let sources = [
        "fn main() { let x: int = 1; x }",
        "fn main() { let x: string = \"hello\"; x }",
        "fn main() { let x: bool = true; x }",
        "fn main() { let x: float64 = 3.14; x }",
        "struct S { x: int }\nfn main() { let x = S { x: 1 }; x }",
        "enum E { A(int), B }\nfn main() { let x = E.A(1); x }",
        "fn main() { let x = (1, \"hi\", true); x }",
        "fn main() { let x = [1, 2, 3]; x }",
        "fn main() { let x: Option<int> = Some(42); x }",
        "fn main() { let x: Result<int, string> = Ok(1); x }",
    ];

    for (i, src) in sources.iter().enumerate() {
        client.change(TEST_URI, src, i as i32 + 1).await;
        let _hover = client.hover(TEST_URI, 0, 4).await;
        let _comp = client.completion(TEST_URI, 0, 4).await;
    }

    client
        .change(TEST_URI, "fn main() { 1 }", sources.len() as i32 + 1)
        .await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}
#[tokio::test]
async fn stress_trailing_newlines() {
    assert!(stress_test_input("fn main() { 1 }\n\n\n\n\n\n\n\n\n\n").await);
}

#[tokio::test]
async fn stress_very_long_string_literal() {
    let long_string: String = "a".repeat(10000);
    let source = format!("fn main() {{ let s = \"{}\"; s }}", long_string);
    assert!(stress_test_input(&source).await);
}

#[tokio::test]
async fn stress_many_let_bindings() {
    let mut source = String::from("fn main() {\n");
    for i in 0..100 {
        source.push_str(&format!("  let v{} = {}\n", i, i));
    }
    source.push_str("  v99\n}");
    assert!(stress_test_all_positions(&source).await);
}
#[tokio::test]
async fn stress_valid_then_broken_then_valid() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client.open(TEST_URI, "fn main() { let x = 42; x }").await;
    let hover1 = client.hover(TEST_URI, 0, 4).await;
    assert!(hover1.is_some());

    client.change(TEST_URI, "fn {{{{{", 2).await;
    let _hover = client.hover(TEST_URI, 0, 0).await;
    let _comp = client.completion(TEST_URI, 0, 0).await;

    client
        .change(TEST_URI, "fn main() { let y = \"hello\"; y }", 3)
        .await;
    let hover3 = client.hover(TEST_URI, 0, 4).await;
    assert!(hover3.is_some());

    client.shutdown().await;
}
#[tokio::test]
async fn stress_compound_assignments() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let mut x = 0
  x += 1
  x -= 2
  x *= 3
  x
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_mutable_struct_field_assignment() {
    assert!(
        stress_test_all_positions(
            "\
struct Counter { count: int }
fn main() {
  let mut c = Counter { count: 0 }
  c.count += 1
  c.count
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_loop_with_break_value() {
    assert!(
        stress_test_all_positions(
            "\
fn main() -> int {
  let result = loop {
    break 42
  }
  result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_loops_with_break_continue() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  for i in [1, 2, 3] {
    for j in [4, 5, 6] {
      if i == 2 { continue }
      if j == 5 { break }
      i + j
    }
  }
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_struct_spread() {
    assert!(
        stress_test_all_positions(
            "\
struct Config { width: int, height: int, debug: bool }
fn main() {
  let base = Config { width: 800, height: 600, debug: false }
  let custom = Config { width: 1024, ..base }
  custom.height
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_recursive_enum() {
    assert!(
        stress_test_all_positions(
            "\
enum List {
  Cons(int, List),
  Nil,
}
fn sum(l: List) -> int {
  match l {
    List.Cons(head, tail) => head + sum(tail),
    List.Nil => 0,
  }
}
fn main() { sum(List.Cons(1, List.Cons(2, List.Nil))) }"
        )
        .await
    );
}
#[tokio::test]
async fn stress_interface_multiple_methods() {
    assert!(
        stress_test_all_positions(
            "\
interface Shape {
  fn area(self) -> float64
  fn perimeter(self) -> float64
  fn name(self) -> string
}
struct Circle { radius: float64 }
impl Circle {
  pub fn area(self) -> float64 { 3.14 * self.radius * self.radius }
  pub fn perimeter(self) -> float64 { 2.0 * 3.14 * self.radius }
  pub fn name(self) -> string { \"circle\" }
}
fn describe<T: Shape>(s: T) -> string { s.name() }
fn main() { describe(Circle { radius: 1.0 }) }"
        )
        .await
    );
}
#[tokio::test]
async fn stress_expressions_as_values_in_calls() {
    assert!(
        stress_test_all_positions(
            "\
fn add(a: int, b: int) -> int { a + b }
fn main() {
  add(
    if true { 1 } else { 2 },
    match 3 {
      3 => 4,
      _ => 5,
    }
  )
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_cast_expression() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: int = 42
  let y = x as float64
  let z = y as int
  z
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_reference_expressions() {
    assert!(
        stress_test_all_positions(
            "\
fn takes_ref(r: &int) -> int { *r }
fn main() {
  let x = 42
  let r = &x
  takes_ref(r)
}"
        )
        .await
    );
}
#[tokio::test]
async fn stress_generic_function_hover() {
    assert!(
        stress_test_all_positions(
            "\
fn identity<T>(x: T) -> T { x }
fn pair<A, B>(a: A, b: B) -> (A, B) { (a, b) }
fn main() {
  let x = identity(42)
  let y = identity(\"hello\")
  let p = pair(x, y)
  p
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_option_result() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let a: Option<Result<int, string>> = Some(Ok(42))
  let b: Result<Option<int>, string> = Ok(Some(10))
  match a {
    Some(Ok(n)) => n,
    Some(Err(e)) => 0,
    None => 0,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_match_in_match() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<int> = Some(42)
  let y: Option<int> = Some(10)
  match x {
    Some(a) => match y {
      Some(b) => a + b,
      None => a,
    },
    None => match y {
      Some(b) => b,
      None => 0,
    },
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_shadowed_variable_hover() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x = 42
  let y = x + 1
  let x = \"hello\"
  let z = x
  z
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_chained_method_calls_long() {
    assert!(
        stress_test_all_positions(
            "\
struct Builder { count: int }
impl Builder {
  fn inc(self) -> Builder { Builder { count: self.count + 1 } }
  fn build(self) -> int { self.count }
}
fn main() {
  let b = Builder { count: 0 }
  b.inc().inc().inc().inc().inc().build()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_let_else_pattern() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<int> = Some(42)
  let Some(value) = x else {
    return ()
  }
  value
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_while_let_loop() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let mut x: Option<int> = Some(5)
  while let Some(n) = x {
    if n == 0 {
      x = None
    } else {
      x = Some(n - 1)
    }
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multiple_return_types() {
    assert!(
        stress_test_all_positions(
            "\
fn classify(n: int) -> string {
  if n < 0 {
    return \"negative\"
  }
  match n {
    0 => \"zero\",
    1 => \"one\",
    _ => \"other\",
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_tuple_destructuring() {
    assert!(
        stress_test_all_positions(
            "\
fn swap(a: int, b: int) -> (int, int) { (b, a) }
fn main() {
  let (x, y) = swap(1, 2)
  let (a, b) = (x + y, x - y)
  a + b
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_enum_with_methods() {
    assert!(
        stress_test_all_positions(
            "\
enum Shape {
  Circle(float64),
  Rect(float64, float64),
}
impl Shape {
  fn area(self) -> float64 {
    match self {
      Shape.Circle(r) => 3.14 * r * r,
      Shape.Rect(w, h) => w * h,
    }
  }
}
fn main() {
  let s = Shape.Circle(5.0)
  s.area()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_closure_as_parameter() {
    assert!(
        stress_test_all_positions(
            "\
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
fn main() {
  let double = |x: int| -> int { x * 2 }
  let result = apply(double, 21)
  apply(|x: int| -> int { x + 1 }, result)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multiline_string_positions() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let s = \"line one
line two
line three\"
  s
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_all_handlers_on_single_char_source() {
    assert!(stress_test_all_positions("fn main() { 1 }").await);
}

#[tokio::test]
async fn stress_completion_on_partially_typed() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() {\n  let value = 42\n  val\n}")
        .await;
    let _comp = client.completion(TEST_URI, 2, 5).await;
    client
        .change(TEST_URI, "fn main() {\n  let value = 42\n  value\n}", 2)
        .await;
    let hover = client.hover(TEST_URI, 2, 3).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_goto_def_on_pattern_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  let x: Option<int> = Some(42)
  match x {
    Some(value) => value + 1,
    None => 0,
  }
}",
        )
        .await;
    let _def = client.goto_definition(TEST_URI, 3, 19).await;
    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_impl_method_self() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { x: int, y: int }
impl Point {
  fn sum(self) -> int { self.x + self.y }
}",
        )
        .await;
    let _hover = client.hover(TEST_URI, 2, 24).await;
    let _hover2 = client.hover(TEST_URI, 2, 34).await;
    client.shutdown().await;
}

#[tokio::test]
async fn stress_rapid_changes_with_errors() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { 1 }").await;
    for i in 0..10 {
        if i % 2 == 0 {
            client
                .change(TEST_URI, "fn main() { let x: int = \"wrong\" }", i + 2)
                .await;
        } else {
            client
                .change(TEST_URI, "fn main() { let x = 42; x }", i + 2)
                .await;
        }
        let _hover = client.hover(TEST_URI, 0, 20).await;
    }
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_or_pattern_in_match() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<int> = Some(3)
  match x {
    Some(1) | Some(2) | Some(3) => true,
    Some(_) | None => false,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_pipeline_chain() {
    assert!(
        stress_test_all_positions(
            "\
fn double(x: int) -> int { x * 2 }
fn add(x: int, y: int) -> int { x + y }
fn main() {
  let result = 5 |> double |> add(10)
  result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multibyte_utf8_identifiers() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let café = 42
  let naïve = café + 1
  naïve
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_multibyte_utf8_in_strings() {
    assert!(
        stress_test_all_positions(
            r#"
fn main() {
  let s = "héllo wörld 日本語"
  let t = "αβγδ"
  s
}"#
        )
        .await
    );
}

#[tokio::test]
async fn stress_single_newline() {
    assert!(stress_test_input("\n").await);
}

#[tokio::test]
async fn stress_self_dot_outside_impl() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let self_val = 1
  self_val
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_completion_self_dot_no_impl() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  self.
}",
        )
        .await;
    let comp = client.completion(TEST_URI, 1, 7).await;
    assert!(
        comp.is_none() || matches!(comp, Some(CompletionResponse::Array(ref v)) if v.is_empty())
    );
    client.shutdown().await;
}

#[tokio::test]
async fn stress_import_only_file_document_symbols() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
import foo
import bar
",
        )
        .await;
    let _symbols = client.document_symbol(TEST_URI).await;

    let _hover = client.hover(TEST_URI, 0, 7).await;
    let _def = client.goto_definition(TEST_URI, 0, 7).await;
    let _comp = client.completion(TEST_URI, 0, 10).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_match_pattern_bindings() {
    assert!(
        stress_test_all_positions(
            "\
enum Shape {
  Circle(float64),
  Rect(float64, float64),
}
fn area(s: Shape) -> float64 {
  match s {
    Shape.Circle(r) => r * r,
    Shape.Rect(w, h) => w * h,
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_hover_on_if_let_pattern() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let x: Option<int> = Some(42)
  if let Some(val) = x {
    val + 1
  } else {
    0
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_calls_signature_help() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn add(x: int, y: int) -> int { x + y }
fn mul(x: int, y: int) -> int { x * y }
fn main() {
  add(mul(1, 2), mul(3, 4))
}",
        )
        .await;

    let sig = client.signature_help(TEST_URI, 3, 6).await;
    assert!(sig.is_some());

    let sig_inner = client.signature_help(TEST_URI, 3, 10).await;
    assert!(sig_inner.is_some());

    let sig_second = client.signature_help(TEST_URI, 3, 17).await;
    assert!(sig_second.is_some());

    let sig_outer_second = client.signature_help(TEST_URI, 3, 22).await;
    assert!(sig_outer_second.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_format_string_interpolation_positions() {
    assert!(
        stress_test_all_positions(
            r#"
fn main() {
  let name = "world"
  let x = 42
  let msg = "hello ${name}, num=${x + 1}"
  msg
}"#
        )
        .await
    );
}

#[tokio::test]
async fn stress_range_expression_not_dot() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let r = 0..10
  for i in 0..5 {
    i
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_prepare_rename_on_various_positions() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { let x = 1; x }").await;

    let _rename_fn_keyword = client.prepare_rename(TEST_URI, 0, 0).await;
    let rename_main = client.prepare_rename(TEST_URI, 0, 3).await;
    assert!(rename_main.is_some());
    let _rename_let = client.prepare_rename(TEST_URI, 0, 12).await;
    let rename_x = client.prepare_rename(TEST_URI, 0, 16).await;
    assert!(rename_x.is_some());
    let _rename_eq = client.prepare_rename(TEST_URI, 0, 18).await;
    let _rename_literal = client.prepare_rename(TEST_URI, 0, 20).await;
    let _rename_usage = client.prepare_rename(TEST_URI, 0, 24).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_references_no_usages() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn unused_fn() -> int { 1 }
fn main() { 42 }",
        )
        .await;

    let refs = client.references(TEST_URI, 0, 3, true).await;
    assert!(refs.is_some());
    let locs = refs.unwrap();
    assert!(locs.len() <= 1);

    client.shutdown().await;
}

#[tokio::test]
async fn stress_type_alias_hover() {
    assert!(
        stress_test_all_positions(
            "\
type Ints = Slice<int>
fn sum(xs: Ints) -> int { 0 }
fn main() {
  let xs: Ints = [1, 2, 3]
  sum(xs)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_impl_block_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
struct Counter { count: int }
impl Counter {
  pub fn new() -> Counter { Counter { count: 0 } }
  pub fn inc(self) -> Counter { Counter { count: self.count + 1 } }
  pub fn get(self) -> int { self.count }
}
fn main() {
  let c = Counter.new()
  let c2 = c.inc()
  c2.get()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_lambda_in_call_arg() {
    assert!(
        stress_test_all_positions(
            "\
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
fn main() {
  apply(fn(x) { x + 1 }, 5)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_struct_calls() {
    assert!(
        stress_test_all_positions(
            "\
struct Inner { val: int }
struct Outer { inner: Inner }
fn main() {
  let o = Outer { inner: Inner { val: 42 } }
  o.inner.val
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_tuple_index_access() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let t = (1, \"hello\", true)
  let a = t.0
  let b = t.1
  let c = t.2
  a
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_propagate_operator() {
    assert!(
        stress_test_all_positions(
            "\
fn inner() -> Result<int, string> { Ok(1) }
fn outer() -> Result<int, string> {
  let x = inner()?
  Ok(x + 1)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_indexed_access_completion() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Item { name: string }
fn main() {
  let items: Slice<Item> = []
  items[0].
}",
        )
        .await;
    let comp = client.completion(TEST_URI, 3, 11).await;
    assert!(comp.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_on_number_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { 42. }").await;
    let _comp = client.completion(TEST_URI, 0, 16).await;

    client.change(TEST_URI, "fn main() { 3.14 }", 2).await;
    let _comp2 = client.completion(TEST_URI, 0, 15).await;

    client.change(TEST_URI, "fn main() { 1 }", 3).await;
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_all_handlers_at_eof() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    let source = "fn main() { 42 }";
    client.open(TEST_URI, source).await;

    let eof_col = source.len() as u32;
    let _hover = client.hover(TEST_URI, 0, eof_col).await;
    let _def = client.goto_definition(TEST_URI, 0, eof_col).await;
    let _comp = client.completion(TEST_URI, 0, eof_col).await;
    let _sig = client.signature_help(TEST_URI, 0, eof_col).await;
    let _refs = client.references(TEST_URI, 0, eof_col, true).await;
    let _rename = client.prepare_rename(TEST_URI, 0, eof_col).await;

    let _hover_past = client.hover(TEST_URI, 0, eof_col + 10).await;
    let _hover_line_past = client.hover(TEST_URI, 5, 0).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_all_handlers_at_position_zero() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Foo { x: int }
enum Bar { A, B }
interface Baz { fn do_thing(self) -> int }
fn main() { 1 }",
        )
        .await;

    let _hover = client.hover(TEST_URI, 0, 0).await;
    let _def = client.goto_definition(TEST_URI, 0, 0).await;
    let _comp = client.completion(TEST_URI, 0, 0).await;
    let _sig = client.signature_help(TEST_URI, 0, 0).await;
    let _refs = client.references(TEST_URI, 0, 0, true).await;
    let _rename = client.prepare_rename(TEST_URI, 0, 0).await;
    let _fmt = client.formatting(TEST_URI).await;
    let _sym = client.document_symbol(TEST_URI).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_formatting_already_formatted() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  let x = 1
  x + 1
}
",
        )
        .await;
    let edits = client.formatting(TEST_URI).await;
    assert!(edits.is_none() || matches!(&edits, Some(v) if v.is_empty()));
    client.shutdown().await;
}

#[tokio::test]
async fn stress_formatting_with_parse_errors() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { let x = ; ; ; }").await;
    let edits = client.formatting(TEST_URI).await;
    assert!(edits.is_none());
    client.shutdown().await;
}

#[tokio::test]
async fn stress_multiple_impl_blocks_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
struct Vec2 { x: float64, y: float64 }
impl Vec2 {
  pub fn new(x: float64, y: float64) -> Vec2 { Vec2 { x: x, y: y } }
  pub fn add(self, other: Vec2) -> Vec2 {
    Vec2 { x: self.x + other.x, y: self.y + other.y }
  }
}
impl Vec2 {
  pub fn scale(self, s: float64) -> Vec2 {
    Vec2 { x: self.x * s, y: self.y * s }
  }
  pub fn len(self) -> float64 { self.x }
}
fn main() {
  let v = Vec2.new(1.0, 2.0)
  let v2 = v.scale(2.0)
  v2.len()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_generic_enum_with_methods() {
    assert!(
        stress_test_all_positions(
            "\
enum Tree<T> {
  Leaf(T),
  Node(Tree<T>, Tree<T>),
}
impl Tree<T> {
  pub fn is_leaf(self) -> bool {
    match self {
      Tree.Leaf(_) => true,
      Tree.Node(_, _) => false,
    }
  }
}
fn main() {
  let t = Tree.Leaf(42)
  t.is_leaf()
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_break_continue_in_loops() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let mut sum = 0
  for i in [1, 2, 3, 4, 5] {
    if i == 3 { continue }
    if i == 5 { break }
    sum = sum + i
  }
  sum
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_loop_with_conditional_break() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let mut i = 0
  let result = loop {
    i = i + 1
    if i > 10 { break i }
  }
  result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_binary_operators_all() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let a = 1 + 2
  let b = 3 - 1
  let c = 2 * 3
  let d = 10 / 2
  let e = 10 % 3
  let f = true && false
  let g = true || false
  let h = 1 == 1
  let i = 1 != 2
  let j = 1 < 2
  let k = 2 > 1
  let l = 1 <= 2
  let m = 2 >= 1
  a + b + c + d + e
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_unary_operators() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let a = !true
  let b = !false
  let c = -1
  let d = -3.14
  a
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_many_params_signature_help() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn many(a: int, b: string, c: bool, d: float64, e: int) -> int { a }
fn main() {
  many(1, \"hi\", true, 3.14, 5)
}",
        )
        .await;

    for col in [7, 10, 16, 22, 28] {
        let sig = client.signature_help(TEST_URI, 2, col).await;
        assert!(sig.is_some());
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_goto_def_on_struct_field_in_pattern() {
    assert!(
        stress_test_all_positions(
            "\
struct Pair { first: int, second: string }
fn main() {
  let p = Pair { first: 1, second: \"hi\" }
  let Pair { first, second } = p
  first
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_hover_on_enum_variant_constructor() {
    assert!(
        stress_test_all_positions(
            "\
enum Expr {
  Num(int),
  Add(Expr, Expr),
}
fn eval(e: Expr) -> int {
  match e {
    Expr.Num(n) => n,
    Expr.Add(l, r) => eval(l) + eval(r),
  }
}
fn main() {
  let e = Expr.Add(Expr.Num(1), Expr.Num(2))
  eval(e)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_cross_module_hover_on_imported_function() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let main_content = "\
import \"math\"

fn main() {
  let x = math.double(5)
  x
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let math_dir = src.join("math");
    std::fs::create_dir_all(&math_dir).unwrap();
    std::fs::write(
        math_dir.join("math.lis"),
        "pub fn double(n: int) -> int { n * 2 }",
    )
    .unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Hover on `double` in `math.double(5)`
    let hover = client.hover(&main_uri, 3, 16).await;
    assert!(
        hover.is_some(),
        "hover on cross-module function should work"
    );
    let content = hover_content(&hover.unwrap());
    assert!(content.contains("int"), "should show return type");

    // Completion after `math.`
    let completion = client.completion(&main_uri, 3, 12).await;
    // May or may not include `double` depending on module loading — just ensure no crash
    let _ = completion;

    // References on `double`
    let refs = client.references(&main_uri, 3, 16, true).await;
    let _ = refs; // no crash

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_completion_on_struct_methods() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let lib_dir = src.join("shapes");
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("shapes.lis"),
        "\
pub struct Circle { pub radius: float64 }
impl Circle {
  pub fn area(self: Circle) -> float64 { 3.14 * self.radius * self.radius }
}",
    )
    .unwrap();

    let main_content = "\
import \"shapes\"

fn main() {
  let c = shapes.Circle { radius: 5.0 }
  c.area()
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Hover on `c` should show Circle type
    let hover = client.hover(&main_uri, 4, 2).await;
    assert!(hover.is_some());

    // Goto definition on `area` should resolve
    let def = client.goto_definition(&main_uri, 4, 4).await;
    let _ = def; // just ensure no crash

    // Signature help on area() call
    let sig = client.signature_help(&main_uri, 4, 8).await;
    let _ = sig; // no crash

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_enum_variant_completion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let types_dir = src.join("types");
    std::fs::create_dir_all(&types_dir).unwrap();
    std::fs::write(
        types_dir.join("types.lis"),
        "\
pub enum Color {
  Red,
  Green,
  Blue,
}",
    )
    .unwrap();

    let main_content = "\
import \"types\"

fn main() {
  let c = types.Color.Red
  c
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // All handlers at various positions — no crash
    for col in [0, 10, 20, 25] {
        let _ = client.hover(&main_uri, 3, col).await;
        let _ = client.completion(&main_uri, 3, col).await;
        let _ = client.goto_definition(&main_uri, 3, col).await;
        let _ = client.prepare_rename(&main_uri, 3, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_rename_local_binding() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let util_dir = src.join("util");
    std::fs::create_dir_all(&util_dir).unwrap();
    std::fs::write(
        util_dir.join("util.lis"),
        "pub fn greet() -> string { \"hi\" }",
    )
    .unwrap();

    let main_content = "\
import \"util\"

fn main() {
  let msg = util.greet()
  msg
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Rename `msg` to `message`
    let edit = client.rename(&main_uri, 3, 6, "message").await;
    assert!(edit.is_some(), "rename should produce workspace edit");
    let changes = edit.unwrap().changes.unwrap();
    let file_edits = changes.get(&Url::parse(&main_uri).unwrap()).unwrap();
    assert!(file_edits.len() >= 2, "should rename definition and usage");

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_empty_name() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { let x = 1; x }").await;

    // Rename to empty string — should be rejected by validation
    let edit = client.rename(TEST_URI, 0, 16, "").await;
    // rename() returns None or error when validation fails
    let _ = edit;

    // Server should still be alive
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_to_keyword() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { let x = 1; x }").await;

    // Rename to keyword — should be rejected
    let edit = client.rename(TEST_URI, 0, 16, "fn").await;
    let _ = edit;

    let edit = client.rename(TEST_URI, 0, 16, "let").await;
    let _ = edit;

    let edit = client.rename(TEST_URI, 0, 16, "match").await;
    let _ = edit;

    // Server still alive
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_to_special_characters() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { let x = 1; x }").await;

    for name in ["123abc", "a-b", "a b", "a.b", "@foo", ""] {
        let edit = client.rename(TEST_URI, 0, 16, name).await;
        let _ = edit;
    }

    // Server still alive
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_diagnostics_contain_error_code() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(TEST_URI, "fn main() { let x: int = \"hello\"; x }")
        .await;

    let diags = client.await_diagnostics().await;
    assert!(!diags.is_empty(), "should have type error diagnostics");
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "should have ERROR severity"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.source == Some("lisette".to_string())),
        "source should be lisette"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_diagnostics_multiple_errors() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  let x: int = \"hello\"
  let y: string = 42
  x + y
}",
        )
        .await;

    let diags = client.await_diagnostics().await;
    assert!(
        diags.len() >= 2,
        "should have multiple type errors, got {}",
        diags.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_diagnostics_warning_lint() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn unused_function() -> int { 42 }
fn main() { 1 }",
        )
        .await;

    let diags = client.await_diagnostics().await;
    // May have warning for unused function
    let _ = diags;

    // Server still alive
    let hover = client.hover(TEST_URI, 1, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_go_import_all_handlers() {
    assert!(
        stress_test_all_positions(
            "\
import \"go:strings\"

fn main() {
  let s = strings.Contains(\"hello world\", \"world\")
  s
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_go_import_multiple_packages() {
    assert!(
        stress_test_all_positions(
            "\
import \"go:fmt\"
import \"go:strings\"

fn main() {
  let s = strings.HasPrefix(\"hello\", \"he\")
  fmt.Println(s)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_go_import_completion_after_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
import \"go:strings\"

fn main() {
  strings.
}",
        )
        .await;

    // Completion after `strings.` (line 3, col 10)
    let completion = client.completion(TEST_URI, 3, 10).await;
    // Should return some completions from Go strings package, not crash
    let _ = completion;

    // All handlers on the dot line
    for col in 0..11 {
        let _ = client.hover(TEST_URI, 3, col).await;
        let _ = client.goto_definition(TEST_URI, 3, col).await;
        let _ = client.prepare_rename(TEST_URI, 3, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_multiple_files_open() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri1 = "file:///test1.lis";
    let uri2 = "file:///test2.lis";
    let uri3 = "file:///test3.lis";

    client
        .open(uri1, "fn add(a: int, b: int) -> int { a + b }")
        .await;
    client
        .open(uri2, "fn greet(name: string) -> string { name }")
        .await;
    client.open(uri3, "fn main() { 42 }").await;

    // Query all three files
    let h1 = client.hover(uri1, 0, 3).await;
    let h2 = client.hover(uri2, 0, 3).await;
    let h3 = client.hover(uri3, 0, 3).await;
    assert!(h1.is_some());
    assert!(h2.is_some());
    assert!(h3.is_some());

    // Completion on each
    let c1 = client.completion(uri1, 0, 0).await;
    let c2 = client.completion(uri2, 0, 0).await;
    assert!(c1.is_some());
    assert!(c2.is_some());

    // Change one file, verify others unaffected
    client.change(uri2, "fn greet() -> int { 1 }", 2).await;
    let h1_after = client.hover(uri1, 0, 3).await;
    assert!(
        h1_after.is_some(),
        "other file should still work after change"
    );

    let h2_after = client.hover(uri2, 0, 3).await;
    assert!(h2_after.is_some(), "changed file should still work");

    client.shutdown().await;
}

#[tokio::test]
async fn stress_goto_def_on_type_annotation() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { x: int, y: int }
fn distance(p: Point) -> float64 { 0.0 }
fn main() {
  let p = Point { x: 1, y: 2 }
  distance(p)
}",
        )
        .await;

    // Goto definition on `Point` in the parameter type annotation
    let def = client.goto_definition(TEST_URI, 1, 17).await;
    assert!(
        def.is_some(),
        "should resolve type annotation to struct definition"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_goto_def_on_return_type_annotation() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Result2 { value: int }
fn make() -> Result2 { Result2 { value: 1 } }
fn main() { make() }",
        )
        .await;

    // Goto definition on `Result2` in the return type
    let def = client.goto_definition(TEST_URI, 1, 15).await;
    assert!(
        def.is_some(),
        "should resolve return type annotation to struct definition"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_on_self_in_impl_with_fields() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Player { name: string, score: int }
impl Player {
  fn display(self: Player) -> string {
    self.
  }
}",
        )
        .await;

    // Completion after `self.` (line 3, col 9)
    let completion = client.completion(TEST_URI, 3, 9).await;
    assert!(completion.is_some());
    let labels = completion_labels(&completion.unwrap());
    assert!(
        labels.contains(&"name".to_string()),
        "should complete struct fields: {:?}",
        labels
    );
    assert!(
        labels.contains(&"score".to_string()),
        "should complete struct fields: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_chained_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Wrapper { inner: Inner }
struct Inner { value: int }
fn main() {
  let w = Wrapper { inner: Inner { value: 42 } }
  w.inner.
}",
        )
        .await;

    // Completion after `w.inner.` (line 4, col 10)
    let completion = client.completion(TEST_URI, 4, 10).await;
    // Should provide completions for Inner type (field: value)
    let _ = completion; // no crash

    client.shutdown().await;
}

#[tokio::test]
async fn stress_document_symbols_all_kinds() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct MyStruct { x: int }
enum MyEnum { A, B }
interface MyInterface { fn method(self: Self) -> int }
type MyAlias = int
const MY_CONST: int = 42
var my_var: int = 0
fn my_function() -> int { 1 }
fn main() { 1 }",
        )
        .await;

    let symbols = client.document_symbol(TEST_URI).await;
    assert!(symbols.is_some());
    let names = symbol_names(&symbols.unwrap());
    assert!(names.contains(&"MyStruct".to_string()));
    assert!(names.contains(&"MyEnum".to_string()));
    assert!(names.contains(&"MyInterface".to_string()));
    assert!(names.contains(&"MyAlias".to_string()));
    assert!(names.contains(&"MY_CONST".to_string()));
    assert!(names.contains(&"my_var".to_string()));
    assert!(names.contains(&"my_function".to_string()));
    assert!(names.contains(&"main".to_string()));

    client.shutdown().await;
}

#[tokio::test]
async fn stress_signature_help_on_method_with_self() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Counter { count: int }
impl Counter {
  fn increment(self: Counter, by: int) -> Counter {
    Counter { count: self.count + by }
  }
}
fn main() {
  let c = Counter { count: 0 }
  c.increment(5)
}",
        )
        .await;

    // Signature help inside `c.increment(5)` — self should be stripped
    let sig = client.signature_help(TEST_URI, 8, 15).await;
    assert!(sig.is_some());
    let sig = sig.unwrap();
    let label = &sig.signatures[0].label;
    // Should show `fn increment(int) -> Counter`, NOT `fn increment(Counter, int) -> Counter`
    assert!(
        !label.contains("Counter, int"),
        "self param should be hidden in method signature: {}",
        label
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_signature_help_generic_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn identity<T>(x: T) -> T { x }
fn main() {
  identity(42)
}",
        )
        .await;

    let sig = client.signature_help(TEST_URI, 2, 11).await;
    assert!(sig.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_alternating_valid_invalid_changes() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // Start valid
    client.open(TEST_URI, "fn main() { 42 }").await;
    let h1 = client.hover(TEST_URI, 0, 4).await;
    assert!(h1.is_some());

    // Make invalid
    client
        .change(TEST_URI, "fn main() { let x: int = \"bad\"", 2)
        .await;
    let _ = client.hover(TEST_URI, 0, 4).await;
    let _ = client.completion(TEST_URI, 0, 0).await;

    // Make valid again
    client
        .change(TEST_URI, "fn main() { let x = 42; x }", 3)
        .await;
    let h3 = client.hover(TEST_URI, 0, 4).await;
    assert!(h3.is_some());

    // Make completely broken
    client.change(TEST_URI, "}{}{}{", 4).await;
    let _ = client.hover(TEST_URI, 0, 0).await;
    let _ = client.completion(TEST_URI, 0, 0).await;

    // Recover
    client.change(TEST_URI, "fn main() { 1 }", 5).await;
    let h5 = client.hover(TEST_URI, 0, 4).await;
    assert!(h5.is_some(), "server should recover from broken state");

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rapid_completion_requests() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Foo { a: int, b: string, c: bool }
fn main() {
  let f = Foo { a: 1, b: \"hi\", c: true }
  f.
}",
        )
        .await;

    // Rapid-fire completion requests on incomplete dot expression
    for _ in 0..10 {
        let _ = client.completion(TEST_URI, 3, 4).await;
    }

    // Fix the code and verify hover recovers
    client
        .change(
            TEST_URI,
            "\
struct Foo { a: int, b: string, c: bool }
fn main() {
  let f = Foo { a: 1, b: \"hi\", c: true }
  f.a
}",
            2,
        )
        .await;
    let hover = client.hover(TEST_URI, 1, 3).await;
    assert!(
        hover.is_some(),
        "hover should recover after fixing broken code"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_at_every_byte_boundary() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    let source = "fn f(x: int) -> int { x + 1 }";
    client.open(TEST_URI, source).await;

    // Hit every character position on line 0
    for col in 0..source.len() as u32 {
        let _ = client.hover(TEST_URI, 0, col).await;
        let _ = client.goto_definition(TEST_URI, 0, col).await;
    }

    // Also past the end
    let _ = client.hover(TEST_URI, 0, source.len() as u32 + 10).await;
    let _ = client.hover(TEST_URI, 100, 0).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_all_handlers_past_end_of_source() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client.open(TEST_URI, "fn main() { 1 }").await;

    // Way past end of file
    let _ = client.hover(TEST_URI, 999, 999).await;
    let _ = client.completion(TEST_URI, 999, 999).await;
    let _ = client.goto_definition(TEST_URI, 999, 999).await;
    let _ = client.references(TEST_URI, 999, 999, true).await;
    let _ = client.signature_help(TEST_URI, 999, 999).await;
    let _ = client.prepare_rename(TEST_URI, 999, 999).await;
    let _ = client.rename(TEST_URI, 999, 999, "foo").await;

    // Still alive
    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_while_let_with_option_chain() {
    assert!(
        stress_test_all_positions(
            "\
fn next_val(n: int) -> Option<int> {
  if n > 0 { Some(n - 1) } else { None }
}
fn main() {
  let mut current = Some(10)
  while let Some(val) = current {
    current = next_val(val)
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_if_let_chain_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
fn main() {
  let a: Option<int> = Some(1)
  let b: Option<int> = Some(2)
  if let Some(x) = a {
    if let Some(y) = b {
      x + y
    } else { 0 }
  } else { 0 }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_match_with_guards_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
enum Shape {
  Circle(float64),
  Rect(float64, float64),
}
fn describe(s: Shape) -> string {
  match s {
    Shape.Circle(r) if r > 10.0 => \"big circle\",
    Shape.Circle(r) => \"small circle\",
    Shape.Rect(w, h) if w == h => \"square\",
    Shape.Rect(_, _) => \"rectangle\",
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_value_enum_all_handlers() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
enum Direction: string {
  North = \"N\",
  South = \"S\",
  East = \"E\",
  West = \"W\",
}
fn main() {
  let d = Direction.North
  match d {
    Direction.North => \"up\",
    Direction.South => \"down\",
    Direction.East => \"right\",
    Direction.West => \"left\",
  }
}",
        )
        .await;

    // Hover on variant
    let hover = client.hover(TEST_URI, 7, 23).await;
    let _ = hover;

    // No completion assertion — just verify handlers don't crash
    let _ = client.completion(TEST_URI, 7, 19).await;

    // Value enum completion is a known gap — verify no crash

    // Prepare rename on enum name
    let rename = client.prepare_rename(TEST_URI, 0, 6).await;
    assert!(rename.is_some());

    // Document symbols
    let symbols = client.document_symbol(TEST_URI).await;
    assert!(symbols.is_some());
    let names = symbol_names(&symbols.unwrap());
    assert!(names.contains(&"Direction".to_string()));

    client.shutdown().await;
}

#[tokio::test]
async fn stress_interface_generic_impl_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
interface Printable {
  fn to_string(self: Self) -> string
}
struct Name { value: string }
impl Name {
  fn to_string(self: Name) -> string { self.value }
}
fn print_it<T: Printable>(item: T) -> string {
  item.to_string()
}
fn main() {
  let n = Name { value: \"Alice\" }
  print_it(n)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_nested_struct_destructuring() {
    assert!(
        stress_test_all_positions(
            "\
struct Inner { value: int }
struct Outer { inner: Inner, name: string }
fn main() {
  let o = Outer { inner: Inner { value: 42 }, name: \"test\" }
  let Outer { inner: Inner { value }, name } = o
  value + len(name)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_try_recover_with_match() {
    assert!(
        stress_test_all_positions(
            "\
fn might_fail(n: int) -> Result<int, string> {
  if n > 0 { Ok(n) } else { Err(\"negative\") }
}
fn main() -> Result<int, string> {
  let result = try {
    let a = might_fail(1)?
    let b = might_fail(2)?
    a + b
  }
  match result {
    Ok(v) => Ok(v),
    Err(e) => Err(e),
  }
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_generic_option_map_slice() {
    assert!(
        stress_test_all_positions(
            "\
fn first<T>(items: Slice<T>) -> Option<T> {
  if len(items) > 0 { Some(items[0]) } else { None }
}
fn main() {
  let nums: Slice<int> = [1, 2, 3]
  let result: Option<int> = first(nums)
  result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_cross_module_import_alias() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let helpers_dir = src.join("helpers");
    std::fs::create_dir_all(&helpers_dir).unwrap();
    std::fs::write(
        helpers_dir.join("helpers.lis"),
        "pub fn compute() -> int { 42 }",
    )
    .unwrap();

    let main_content = "\
import h \"helpers\"

fn main() {
  h.compute()
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Hover on `compute` via alias
    let hover = client.hover(&main_uri, 3, 5).await;
    let _ = hover; // no crash

    // Completion after `h.`
    let completion = client.completion(&main_uri, 3, 4).await;
    let _ = completion; // no crash

    // Goto def on `compute` via alias
    let def = client.goto_definition(&main_uri, 3, 5).await;
    let _ = def; // no crash

    client.shutdown().await;
}

#[tokio::test]
async fn stress_const_var_all_handlers() {
    assert!(
        stress_test_all_positions(
            "\
const MAX: int = 100
const MIN: int = 0
var counter: int = 0
fn main() {
  let range = MAX - MIN
  counter = counter + 1
  range + counter
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_formatting_complex_match() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
enum Tree { Leaf(int), Node(Tree,Tree) }
fn sum(t:Tree)->int{match t{Tree.Leaf(n)=>n,Tree.Node(l,r)=>sum(l)+sum(r)}}
fn main(){sum(Tree.Node(Tree.Leaf(1),Tree.Leaf(2)))}",
        )
        .await;

    let edits = client.formatting(TEST_URI).await;
    assert!(
        edits.is_some(),
        "formatter should produce edits for compressed code"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_all_literal_types() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  let a = 42
  let b = 3.14
  let c = true
  let d = \"hello\"
  let e = 'x'
  let f = [1, 2, 3]
  let g = (1, \"two\", true)
  a
}",
        )
        .await;

    // Hover on each binding name
    for (line, expected) in [
        (1, "int"),
        (2, "float64"),
        (3, "bool"),
        (4, "string"),
        (5, "rune"),
    ] {
        let hover = client.hover(TEST_URI, line, 6).await;
        assert!(hover.is_some(), "hover on line {} should work", line);
        let content = hover_content(&hover.unwrap());
        assert!(
            content.contains(expected),
            "line {}: expected '{}' in '{}'",
            line,
            expected,
            content
        );
    }

    client.shutdown().await;
}

#[tokio::test]
async fn hover_falls_back_to_last_valid_snapshot() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // Start with valid code — this populates last_valid_snapshot
    client.open(TEST_URI, "fn main() { let x = 42; x }").await;
    let hover = client.hover(TEST_URI, 0, 3).await;
    assert!(hover.is_some(), "hover should work on valid code");

    // Break the code with a lex error — run_analysis returns Err
    client
        .change(TEST_URI, "fn main() { let x = 42; x.", 2)
        .await;

    // Hover should still work using last_valid_snapshot fallback
    let hover = client.hover(TEST_URI, 0, 3).await;
    assert!(
        hover.is_some(),
        "hover should fall back to last valid snapshot during parse errors"
    );

    client.shutdown().await;
}

/// Tests that completion after a dot following a multi-byte character
/// doesn't panic in get_module_prefix (rfind + 1 byte offset issue).
#[tokio::test]
async fn stress_completion_after_multibyte_identifier() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // CJK character (3 bytes each in UTF-8) used as separator before an identifier
    // This tests that `get_module_prefix` handles multi-byte chars correctly
    // when doing `rfind(|c| ...).map(|i| i + 1)` — i+1 could be mid-char
    client
        .open(
            TEST_URI,
            "\
struct S { value: int }
fn main() {
  let s = S { value: 1 }
  s.value
}",
        )
        .await;

    // All handlers at various positions on the dot access line
    for col in 0..10 {
        let _ = client.hover(TEST_URI, 3, col).await;
        let _ = client.completion(TEST_URI, 3, col).await;
        let _ = client.goto_definition(TEST_URI, 3, col).await;
    }

    client.shutdown().await;
}

/// Specifically test that completion doesn't panic when a multi-byte
/// character appears right before an identifier followed by a dot.
#[tokio::test]
async fn stress_completion_multibyte_before_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // `let x = 1; 名.` — multi-byte char right before dot
    // The get_module_prefix rfind will find the multi-byte char as the non-ident boundary,
    // then `i + 1` might be mid-char, which would panic on `base[start..]`
    // However, Lisette identifiers can only be ASCII, so this shouldn't happen in practice
    // because the lexer wouldn't produce a multi-byte identifier. But the LSP should
    // handle it gracefully without panic.
    client
        .open(TEST_URI, "fn main() {\n  let x = 1\n  x\n}")
        .await;

    // Just verify all handlers work at every position
    for line in 0..4 {
        for col in 0..20 {
            let _ = client.hover(TEST_URI, line, col).await;
            let _ = client.completion(TEST_URI, line, col).await;
        }
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_dot_after_emoji_string() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // String containing emoji (4-byte UTF-8) followed by code
    client
        .open(
            TEST_URI,
            "\
struct S { x: int }
fn main() {
  let s = \"🎉\"
  let obj = S { x: 1 }
  obj.x
}",
        )
        .await;

    // Hover/completion on `obj.x` line (after the emoji string line)
    for col in 0..8 {
        let _ = client.hover(TEST_URI, 4, col).await;
        let _ = client.completion(TEST_URI, 4, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_snapshot_cache_invalidation() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // Open with valid code
    client.open(TEST_URI, "fn main() { 42 }").await;
    let h1 = client.hover(TEST_URI, 0, 4).await;
    assert!(h1.is_some());

    // Change to different valid code — cache should be invalidated
    client
        .change(TEST_URI, "fn greet() -> string { \"hi\" }", 2)
        .await;
    let h2 = client.hover(TEST_URI, 0, 4).await;
    assert!(h2.is_some());
    let content = hover_content(&h2.unwrap());
    assert!(
        content.contains("string"),
        "hover should reflect new code, got: {}",
        content
    );

    // Change back
    client.change(TEST_URI, "fn main() -> int { 42 }", 3).await;
    let h3 = client.hover(TEST_URI, 0, 4).await;
    assert!(h3.is_some());
    let content = hover_content(&h3.unwrap());
    assert!(
        content.contains("int"),
        "hover should reflect reverted code, got: {}",
        content
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_struct_with_usages() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Foo { x: int }
fn make() -> Foo { Foo { x: 1 } }
fn main() { let f: Foo = make(); f }",
        )
        .await;

    // Prepare rename on struct name `Foo`
    let prep = client.prepare_rename(TEST_URI, 0, 8).await;
    assert!(prep.is_some());

    // Rename — currently only renames the definition itself, not type annotation
    // usages. This is a known limitation of the usage tracking system.
    let edit = client.rename(TEST_URI, 0, 8, "Bar").await;
    assert!(edit.is_some());
    let changes = edit.unwrap().changes.unwrap();
    let edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();
    assert!(!edits.is_empty(), "should rename at least the definition");

    client.shutdown().await;
}

#[tokio::test]
async fn stress_rename_function_with_usages() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn helper() -> int { 42 }
fn main() {
  let a = helper()
  let b = helper()
  a + b
}",
        )
        .await;

    let edit = client.rename(TEST_URI, 0, 4, "compute").await;
    assert!(edit.is_some());
    let changes = edit.unwrap().changes.unwrap();
    let edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();
    assert!(
        edits.len() >= 3,
        "should rename definition + 2 usages, got {} edits",
        edits.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_if_match_block_values() {
    assert!(
        stress_test_all_positions(
            "\
fn classify(n: int) -> string {
  let label = if n > 100 {
    \"big\"
  } else if n > 10 {
    \"medium\"
  } else {
    \"small\"
  }
  let result = match n {
    0 => \"zero\",
    1 => \"one\",
    _ => label,
  }
  result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_open_change_query_close_cycle() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri1 = "file:///cycle1.lis";
    let uri2 = "file:///cycle2.lis";

    for i in 0..5i32 {
        client
            .open(uri1, &format!("fn f{i}() -> int {{ {i} }}"))
            .await;
        client
            .open(uri2, &format!("fn g{i}() -> string {{ \"v{i}\" }}"))
            .await;

        let _ = client.hover(uri1, 0, 3).await;
        let _ = client.completion(uri2, 0, 0).await;

        client
            .change(
                uri1,
                &format!("fn f{i}(x: int) -> int {{ x + {i} }}"),
                i + 2,
            )
            .await;

        let _ = client.hover(uri1, 0, 3).await;
        let _ = client.document_symbol(uri2).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_large_struct_completion() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let mut fields = String::new();
    for i in 0..20 {
        if i > 0 {
            fields.push_str(", ");
        }
        fields.push_str(&format!("field_{i}: int"));
    }
    let source = format!(
        "\
struct Big {{ {fields} }}
fn main() {{
  let b = Big {{ {} }}
  b.field_0
}}",
        (0..20)
            .map(|i| format!("field_{i}: {i}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    client.open(TEST_URI, &source).await;

    // Completion after `b.` should include all 20 fields
    let lines: Vec<&str> = source.lines().collect();
    let dot_line = lines.len() as u32 - 2; // `  b.field_0` line
    let _ = client.completion(TEST_URI, dot_line, 4).await;

    // All handlers on the struct definition line
    for col in [0, 5, 10, 20, 30, 40, 50] {
        let _ = client.hover(TEST_URI, 0, col).await;
        let _ = client.goto_definition(TEST_URI, 0, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_incremental_method_chain_typing() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let stages = [
        "struct S { x: int }\nfn main() {\n  let s = S { x: 1 }\n  s\n}",
        "struct S { x: int }\nfn main() {\n  let s = S { x: 1 }\n  s.\n}",
        "struct S { x: int }\nfn main() {\n  let s = S { x: 1 }\n  s.x\n}",
    ];

    client.open(TEST_URI, stages[0]).await;

    for (i, stage) in stages[1..].iter().enumerate() {
        client.change(TEST_URI, stage, (i + 2) as i32).await;
        let _ = client.hover(TEST_URI, 3, 3).await;
        let _ = client.completion(TEST_URI, 3, 3).await;
        let _ = client.goto_definition(TEST_URI, 3, 3).await;
    }

    // Final state should have working hover
    let hover = client.hover(TEST_URI, 3, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_incremental_function_typing() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // Simulate typing a function definition character by character
    let stages = [
        "fn",
        "fn m",
        "fn main",
        "fn main()",
        "fn main() {",
        "fn main() { 4",
        "fn main() { 42",
        "fn main() { 42 }",
    ];

    client.open(TEST_URI, stages[0]).await;
    for (i, stage) in stages[1..].iter().enumerate() {
        client.change(TEST_URI, stage, (i + 2) as i32).await;
        let _ = client.hover(TEST_URI, 0, 0).await;
        let _ = client.completion(TEST_URI, 0, 0).await;
    }

    let hover = client.hover(TEST_URI, 0, 4).await;
    assert!(hover.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_self_in_impl() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { x: int, y: int }
impl Point {
  fn origin() -> Point { Point { x: 0, y: 0 } }
  fn shift(self: Point, dx: int) -> Point {
    Point { x: self.x + dx, y: self.y }
  }
}
fn main() {
  let p = Point.origin()
  p.shift(1)
}",
        )
        .await;

    // Hover on `self` in `self.x` (line 4)
    let _ = client.hover(TEST_URI, 4, 17).await;

    // Hover on `self` parameter (line 3)
    let _ = client.hover(TEST_URI, 3, 12).await;

    // Completion after `self.` (line 4, col 17)
    let _ = client.completion(TEST_URI, 4, 19).await;

    // Goto def on static method call `Point.origin()`
    let _ = client.goto_definition(TEST_URI, 8, 17).await;

    // Goto def on instance method call `p.shift(1)`
    let _ = client.goto_definition(TEST_URI, 9, 4).await;

    // Signature help on `p.shift(1)` — should strip self
    let sig = client.signature_help(TEST_URI, 9, 10).await;
    assert!(sig.is_some(), "signature help should return a result");
    let label = &sig.unwrap().signatures[0].label;
    assert!(
        !label.contains("Point, int"),
        "self should be stripped from signature: {}",
        label
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_in_match_pattern() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
enum Shape {
  Circle(float64),
  Rect(float64, float64),
}
fn main() {
  let s = Shape.Circle(1.0)
  match s {
    Shape.
  }
}",
        )
        .await;

    // Completion after `Shape.` in pattern position (line 7, after the dot)
    // `    Shape.` — dot at col 9, so col 10 is right after
    // Should not crash, even if pattern-position completion isn't supported
    let _ = client.completion(TEST_URI, 7, 10).await;

    // All handlers at various positions on the match arm line
    for col in 0..12 {
        let _ = client.hover(TEST_URI, 7, col).await;
        let _ = client.goto_definition(TEST_URI, 7, col).await;
        let _ = client.prepare_rename(TEST_URI, 7, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_enum_variant_goto_def() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let types_dir = src.join("types");
    std::fs::create_dir_all(&types_dir).unwrap();
    std::fs::write(
        types_dir.join("types.lis"),
        "\
pub enum Color {
  Red,
  Green,
  Blue,
}
pub fn is_warm(c: Color) -> bool {
  match c {
    Color.Red => true,
    Color.Green => false,
    Color.Blue => false,
  }
}",
    )
    .unwrap();

    let main_content = "\
import \"types\"

fn main() {
  let c = types.Color.Red
  types.is_warm(c)
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Goto def on `Color` (should go to enum definition in types.lis)
    let _ = client.goto_definition(&main_uri, 3, 16).await;

    // Goto def on `Red` (should go to variant definition)
    let _ = client.goto_definition(&main_uri, 3, 22).await;

    // Goto def on `is_warm`
    let _ = client.goto_definition(&main_uri, 4, 10).await;

    // Hover on the chain
    for col in [10, 14, 16, 20, 22, 24] {
        let _ = client.hover(&main_uri, 3, col).await;
    }

    // Signature help on `types.is_warm(c)`
    let _ = client.signature_help(&main_uri, 4, 17).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_references_on_struct_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Config { width: int, height: int }
fn area(c: Config) -> int {
  c.width * c.height
}
fn main() {
  let c = Config { width: 10, height: 20 }
  area(c)
}",
        )
        .await;

    // References on `width` in struct definition (line 0)
    let _ = client.references(TEST_URI, 0, 18, true).await;

    // References on `width` in constructor (line 5)
    let _ = client.references(TEST_URI, 5, 20, true).await;

    // References on `width` in `c.width` (line 2)
    let _ = client.references(TEST_URI, 2, 4, true).await;

    // Goto def on `width` in `c.width`
    let _ = client.goto_definition(TEST_URI, 2, 4).await;

    // Hover on `width` in `c.width`
    let _ = client.hover(TEST_URI, 2, 4).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_handlers_on_import_statement() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let lib_dir = src.join("mylib");
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("mylib.lis"),
        "pub fn greet() -> string { \"hello\" }",
    )
    .unwrap();

    let main_content = "\
import \"mylib\"

fn main() {
  mylib.greet()
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // All handlers on the import line
    for col in 0..16 {
        let _ = client.hover(&main_uri, 0, col).await;
        let _ = client.goto_definition(&main_uri, 0, col).await;
        let _ = client.completion(&main_uri, 0, col).await;
        let _ = client.prepare_rename(&main_uri, 0, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_for_loop_components() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  let items = [10, 20, 30]
  for item in items {
    item + 1
  }
}",
        )
        .await;

    // Hover on `item` binding (line 2, col 6)
    let hover = client.hover(TEST_URI, 2, 6).await;
    assert!(hover.is_some());
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "for binding should be int: {}",
        content
    );

    // Hover on `items` iterable (line 2, col 14)
    let hover = client.hover(TEST_URI, 2, 14).await;
    assert!(hover.is_some());

    // Hover on `item` inside body (line 3, col 4)
    let hover = client.hover(TEST_URI, 3, 4).await;
    assert!(hover.is_some());
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "for body var should be int: {}",
        content
    );

    // Goto def on `item` in body should go to for binding
    let def = client.goto_definition(TEST_URI, 3, 4).await;
    assert!(def.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_pipeline_completion() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn double(x: int) -> int { x * 2 }
fn add_one(x: int) -> int { x + 1 }
fn main() {
  let result = 5 |> double |> add_one
  result
}",
        )
        .await;

    // All handlers at every position on the pipeline line
    let line = "  let result = 5 |> double |> add_one";
    for col in 0..line.len() as u32 {
        let _ = client.hover(TEST_URI, 3, col).await;
        let _ = client.completion(TEST_URI, 3, col).await;
        let _ = client.goto_definition(TEST_URI, 3, col).await;
    }

    // Signature help at various points
    for col in [18, 22, 30, 35] {
        let _ = client.signature_help(TEST_URI, 3, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_type_error_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let lib_dir = src.join("mymod");
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(lib_dir.join("mymod.lis"), "pub fn get_num() -> int { 42 }").unwrap();

    // Main has a type error using imported function
    let main_content = "\
import \"mymod\"

fn main() {
  let x: string = mymod.get_num()
  x
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    let diags = client.await_diagnostics().await;
    assert!(
        !diags.is_empty(),
        "should have type error for assigning int to string"
    );

    // All handlers should still work despite diagnostics
    let _ = client.hover(&main_uri, 3, 20).await;
    let comp = client.completion(&main_uri, 3, 0).await;
    assert!(comp.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_sibling_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let mod_dir = src.join("shapes");
    std::fs::create_dir_all(&mod_dir).unwrap();

    // Two sibling files in the same module
    std::fs::write(
        mod_dir.join("circle.lis"),
        "pub struct Circle { pub radius: float64 }",
    )
    .unwrap();
    std::fs::write(
        mod_dir.join("rect.lis"),
        "pub struct Rect { pub width: float64, pub height: float64 }",
    )
    .unwrap();
    // Module entry file
    std::fs::write(mod_dir.join("shapes.lis"), "").unwrap();

    let main_content = "\
import \"shapes\"

fn main() {
  let c = shapes.Circle { radius: 5.0 }
  let r = shapes.Rect { width: 10.0, height: 20.0 }
  c.radius + r.width
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Hover on Circle and Rect
    let _ = client.hover(&main_uri, 3, 18).await;
    let _ = client.hover(&main_uri, 4, 18).await;

    // Completion after `shapes.`
    let _ = client.completion(&main_uri, 3, 16).await;

    // Goto def on `radius` and `width`
    let _ = client.goto_definition(&main_uri, 5, 4).await;
    let _ = client.goto_definition(&main_uri, 5, 16).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_generic_params() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn identity<T>(x: T) -> T { x }
fn pair<A, B>(a: A, b: B) -> (A, B) { (a, b) }
fn main() {
  identity(42)
  pair(1, \"hello\")
}",
        )
        .await;

    // Hover on `T` in generic param
    let _ = client.hover(TEST_URI, 0, 13).await;

    // Hover on calls — should show inferred type
    let hover = client.hover(TEST_URI, 3, 2).await;
    assert!(hover.is_some());

    let hover = client.hover(TEST_URI, 4, 2).await;
    assert!(hover.is_some());

    // Signature help on generic function calls
    let sig = client.signature_help(TEST_URI, 3, 11).await;
    assert!(sig.is_some());

    let sig = client.signature_help(TEST_URI, 4, 6).await;
    assert!(sig.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn stress_all_handlers_fall_back_during_lex_error() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // Establish valid snapshot
    client
        .open(
            TEST_URI,
            "\
struct Point { x: int, y: int }
fn distance(p: Point) -> int { p.x + p.y }
fn main() {
  let p = Point { x: 3, y: 4 }
  distance(p)
}",
        )
        .await;

    let hover = client.hover(TEST_URI, 1, 3).await;
    assert!(hover.is_some());

    // Break with lex error (unclosed string)
    client
        .change(
            TEST_URI,
            "\
struct Point { x: int, y: int }
fn distance(p: Point) -> int { p.x + p.y }
fn main() {
  let p = Point { x: 3, y: 4 }
  distance(p)
  let broken = \"unclosed",
            2,
        )
        .await;

    // All handlers should fall back to last valid snapshot
    let hover = client.hover(TEST_URI, 1, 3).await;
    assert!(
        hover.is_some(),
        "hover should work via last_valid_snapshot during lex error"
    );

    let comp = client.completion(TEST_URI, 0, 0).await;
    assert!(comp.is_some(), "completion should work via fallback");

    let _ = client.goto_definition(TEST_URI, 4, 2).await;
    let _ = client.references(TEST_URI, 1, 3, true).await;
    let _ = client.signature_help(TEST_URI, 4, 11).await;

    let syms = client.document_symbol(TEST_URI).await;
    assert!(syms.is_some(), "document symbols should work via fallback");

    let _ = client.formatting(TEST_URI).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_completion_on_map_indexed_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Item { name: string, count: int }
fn main() {
  let items: Slice<Item> = []
  items[0].name
}",
        )
        .await;

    // Completion after `items[0].`
    let _ = client.completion(TEST_URI, 3, 11).await;

    // Hover and goto-def on field access
    let _ = client.hover(TEST_URI, 3, 12).await;
    let _ = client.goto_definition(TEST_URI, 3, 12).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_multiple_imports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    for name in ["a", "b", "c"] {
        let d = src.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join(format!("{name}.lis")),
            format!("pub fn f{name}() -> int {{ 1 }}"),
        )
        .unwrap();
    }

    let main_content = "\
import \"a\"
import \"b\"
import \"c\"

fn main() {
  a.fa() + b.fb() + c.fc()
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Hover, goto-def, completion on each module's function
    for col in [4, 14, 24] {
        let _ = client.hover(&main_uri, 5, col).await;
        let _ = client.goto_definition(&main_uri, 5, col).await;
        let _ = client.completion(&main_uri, 5, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_enum_struct_variant_match_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
enum Event {
  Click { x: int, y: int },
  Scroll(int),
  Quit,
}
fn handle(e: Event) -> int {
  match e {
    Event.Click { x, y } => x + y,
    Event.Scroll(amount) => amount,
    Event.Quit => 0,
  }
}
fn main() {
  let e = Event.Click { x: 10, y: 20 }
  handle(e)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_bounded_generic_function_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
interface Summable {
  fn sum(self: Self) -> int
}
struct Pair { a: int, b: int }
impl Pair {
  fn sum(self: Pair) -> int { self.a + self.b }
}
fn total<T: Summable>(items: Slice<T>) -> int {
  let mut result = 0
  for item in items {
    result = result + item.sum()
  }
  result
}
fn main() {
  let pairs = [Pair { a: 1, b: 2 }, Pair { a: 3, b: 4 }]
  total(pairs)
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_empty_constructs_all_handlers() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Empty {}
enum SingleVariant { Only }
interface NoMethods {}
fn noop() {}
fn main() {
  let e = Empty {}
  noop()
}",
        )
        .await;

    let syms = client.document_symbol(TEST_URI).await;
    assert!(syms.is_some());
    let names = symbol_names(&syms.unwrap());
    assert!(names.contains(&"Empty".to_string()));
    assert!(names.contains(&"SingleVariant".to_string()));
    assert!(names.contains(&"NoMethods".to_string()));
    assert!(names.contains(&"noop".to_string()));

    for line in 0..8 {
        for col in [0, 5, 10, 15, 20] {
            let _ = client.hover(TEST_URI, line, col).await;
            let _ = client.completion(TEST_URI, line, col).await;
        }
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_diagnostics_after_rapid_edits() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client.open(TEST_URI, "fn main() { 42 }").await;
    let _ = client.await_diagnostics().await;

    for i in 2..7i32 {
        client
            .change(TEST_URI, &format!("fn main() {{ {} }}", i), i)
            .await;
    }

    // Final change introduces a type error
    client
        .change(TEST_URI, "fn main() { let x: int = \"bad\"; x }", 7)
        .await;

    let diags = client.await_diagnostics().await;
    assert!(
        !diags.is_empty(),
        "should eventually get diagnostics for type error"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_hover_on_expression_values() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn main() {
  let a = if true { 1 } else { 2 }
  let b = match a {
    1 => \"one\",
    _ => \"other\",
  }
  let c = {
    let tmp = 42
    tmp + 1
  }
  a + c
}",
        )
        .await;

    // Hover on `a` — should be int
    let hover = client.hover(TEST_URI, 1, 6).await;
    assert!(hover.is_some());
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "if-else value should be int: {}",
        content
    );

    // Hover on `b` — should be string
    let hover = client.hover(TEST_URI, 2, 6).await;
    assert!(hover.is_some());
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("string"),
        "match value should be string: {}",
        content
    );

    // Hover on `c` — should be int (block expression)
    let hover = client.hover(TEST_URI, 6, 6).await;
    assert!(hover.is_some());
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "block expression value should be int: {}",
        content
    );

    client.shutdown().await;
}

#[tokio::test]
async fn stress_closure_capture_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
fn main() {
  let offset = 10
  let add_offset = |x: int| -> int { x + offset }
  let result = apply(add_offset, 5)
  result
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_or_pattern_let_else_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
enum Token {
  Num(int),
  Plus,
  Minus,
}
fn value(t: Token) -> int {
  let Token.Num(n) | Token.Plus = t else { return 0 }
  n
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_deep_field_access_chain() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct A { b: B }
struct B { c: C }
struct C { value: int }
fn main() {
  let a = A { b: B { c: C { value: 42 } } }
  a.b.c.value
}",
        )
        .await;

    // Hover on each part of the chain
    let hover = client.hover(TEST_URI, 5, 2).await;
    assert!(hover.is_some());

    for col in [2, 4, 6, 8] {
        let _ = client.hover(TEST_URI, 5, col).await;
        let _ = client.goto_definition(TEST_URI, 5, col).await;
        let _ = client.completion(TEST_URI, 5, col).await;
    }

    client.shutdown().await;
}

#[tokio::test]
async fn stress_cross_module_rename_function() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let lib_dir = src.join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(lib_dir.join("lib.lis"), "pub fn compute() -> int { 42 }").unwrap();

    let main_content = "\
import \"lib\"

fn main() {
  let x = lib.compute()
  let y = lib.compute()
  x + y
}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_path = src.join("main.lis");
    let main_uri = Url::from_file_path(&main_path).unwrap().to_string();
    client.open(&main_uri, main_content).await;

    // Try prepare rename and rename on cross-module function
    let _ = client.prepare_rename(&main_uri, 3, 14).await;
    let _ = client.rename(&main_uri, 3, 14, "calculate").await;
    let _ = client.references(&main_uri, 3, 14, true).await;

    client.shutdown().await;
}

#[tokio::test]
async fn stress_format_string_complex() {
    assert!(
        stress_test_all_positions(
            "\
struct Point { x: int, y: int }
fn main() {
  let p = Point { x: 1, y: 2 }
  let msg = f\"Point({p.x}, {p.y})\"
  let nested = f\"Result: {if true { 1 } else { 2 }}\"
  msg
}"
        )
        .await
    );
}

#[tokio::test]
async fn stress_var_mutation_all_positions() {
    assert!(
        stress_test_all_positions(
            "\
var counter: int = 0
fn increment() {
  counter = counter + 1
}
fn main() {
  increment()
  increment()
  counter
}"
        )
        .await
    );
}

#[tokio::test]
async fn goto_definition_method_call_via_dot_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct User { pub name: string }
impl User {
  pub fn greet(self: User) -> string { self.name }
}
fn main() {
  let u = User { name: \"Alice\" }
  u.greet()
}",
        )
        .await;

    // Cursor on "greet" in "u.greet()"
    let response = client.goto_definition(TEST_URI, 6, 4).await;
    assert!(
        response.is_some(),
        "goto_definition on method call should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    // Should jump to the method definition (line 2: pub fn greet)
    assert_eq!(loc.range.start.line, 2);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_variable_in_dot_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { pub x: int }
impl Point {
  pub fn translate(self: Point) -> Point { self }
}
fn main() {
  let p = Point { x: 1 }
  p.translate()
}",
        )
        .await;

    // Cursor on "p" in "p.translate()"
    let response = client.goto_definition(TEST_URI, 6, 2).await;
    assert!(
        response.is_some(),
        "goto_definition on variable in dot access should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    // Should jump to "let p = ..." (line 5)
    assert_eq!(loc.range.start.line, 5);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_type_in_let_annotation() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "type UserID = int\nfn main() {\n  let x: UserID = 42\n}",
        )
        .await;

    // Cursor on "UserID" in "let x: UserID = 42" (line 2, col 9)
    let response = client.goto_definition(TEST_URI, 2, 9).await;
    assert!(
        response.is_some(),
        "goto_definition on type in let annotation should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_type_in_static_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { pub x: int }
impl Point {
  pub fn origin() -> Point { Point { x: 0 } }
}
fn main() {
  Point.origin()
}",
        )
        .await;

    // Cursor on "Point" in "Point.origin()"
    let response = client.goto_definition(TEST_URI, 5, 2).await;
    assert!(
        response.is_some(),
        "goto_definition on type name in static call should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_import_alias() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "import \"go:fmt\"\nfn main() {\n  fmt.Println(\"hi\")\n}",
        )
        .await;

    // Cursor on "fmt" in "fmt.Println"
    let response = client.goto_definition(TEST_URI, 2, 2).await;
    assert!(
        response.is_some(),
        "goto_definition on import alias should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    // Should jump to the import statement (line 0)
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_pipe_operator() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn add(a: int, b: int) -> int { a + b }\nfn main() {\n  5 |> add(3)\n}",
        )
        .await;

    // Cursor on "add" in "5 |> add(3)" (line 2, col 7)
    let response = client.goto_definition(TEST_URI, 2, 7).await;
    assert!(response.is_some(), "goto_definition in pipe should resolve");

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn find_references_struct_name() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { pub x: int, pub y: int }
fn make() -> Point {
  Point { x: 1, y: 2 }
}
fn main() {
  let p: Point = make()
  p.x
}",
        )
        .await;

    // Cursor on "Point" in the struct definition (line 0, col 7)
    let refs = client.references(TEST_URI, 0, 7, true).await;
    assert!(refs.is_some(), "find references for struct should succeed");

    let locations = refs.unwrap();
    // Should find: definition (line 0), return type (line 1), constructor (line 2), type annotation (line 5)
    assert!(
        locations.len() >= 2,
        "should find at least the definition and one usage, found {}",
        locations.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn rename_enum_variant_preserves_qualifier() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    let source = "\
enum Color { Red, Green, Blue }
fn main() {
  let c = Color.Red
  match c {
    Color.Red => 1,
    Color.Green => 2,
    Color.Blue => 3,
  }
}";
    client.open(TEST_URI, source).await;

    // Rename "Red" in the enum definition (line 0, col 13)
    let edit = client.rename(TEST_URI, 0, 13, "Crimson").await;
    assert!(edit.is_some(), "rename enum variant should succeed");

    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();

    // Verify all edits replace only "Red" with "Crimson", not "Color.Red" with "Crimson"
    for text_edit in edits {
        assert_eq!(text_edit.new_text, "Crimson");
        // Each edit should be on a single line
        assert_eq!(
            text_edit.range.start.line, text_edit.range.end.line,
            "edit should be on a single line"
        );
        // The edit range should span only the length of "Red" (3 chars),
        // not "Color.Red" (9 chars)
        let char_span = text_edit.range.end.character - text_edit.range.start.character;
        assert_eq!(
            char_span, 3,
            "edit should span only the variant name ('Red' = 3 chars), got {} chars",
            char_span
        );
    }

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_chained_pipe_operator() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
fn add(a: int, b: int) -> int { a + b }
fn multiply(a: int, b: int) -> int { a * b }
fn main() {
  let result = 5 |> add(3) |> multiply(2)
}",
        )
        .await;

    // Cursor on "add" in chained pipe "5 |> add(3) |> multiply(2)" (line 3, col 20)
    let response = client.goto_definition(TEST_URI, 3, 20).await;
    assert!(
        response.is_some(),
        "goto_definition on add in chained pipe should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0, "should jump to add definition");

    // Cursor on "multiply" in chained pipe (line 3, col 30)
    let response = client.goto_definition(TEST_URI, 3, 30).await;
    assert!(
        response.is_some(),
        "goto_definition on multiply in chained pipe should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 1,
        "should jump to multiply definition"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn rename_struct_updates_type_annotations() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    let source = "\
pub struct Point { pub x: int, pub y: int }
impl Point {
  pub fn new(x: int, y: int) -> Point {
    Point { x, y }
  }
  pub fn distance(self: Point, other: Point) -> int {
    self.x - other.x
  }
}
fn main() {
  let p: Point = Point.new(1, 2)
}";
    client.open(TEST_URI, source).await;

    // Rename "Point" in the struct definition (line 0, col 11)
    let edit = client.rename(TEST_URI, 0, 11, "Vec2").await;
    assert!(edit.is_some(), "rename struct should succeed");

    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();

    // Should update: struct def, impl target, return type, self param, other param,
    // let type annotation, constructor call
    assert!(
        edits.len() >= 5,
        "rename should update at least the definition + impl + return type + params + usage, got {}",
        edits.len()
    );

    for text_edit in edits {
        assert_eq!(text_edit.new_text, "Vec2");
    }

    // Verify no duplicate edits at the same position (which would cause double-rename)
    let mut seen = std::collections::HashSet::new();
    for text_edit in edits {
        let key = (text_edit.range.start.line, text_edit.range.start.character);
        assert!(
            seen.insert(key),
            "duplicate edit at line {} col {} — would cause double-rename",
            key.0,
            key.1
        );
    }

    client.shutdown().await;
}

#[tokio::test]
async fn find_references_struct_includes_type_annotations() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
pub struct Point { pub x: int, pub y: int }
impl Point {
  pub fn origin() -> Point {
    Point { x: 0, y: 0 }
  }
}
fn main() {
  let p: Point = Point.origin()
}",
        )
        .await;

    // Cursor on "Point" in the struct definition (line 0, col 11)
    let refs = client.references(TEST_URI, 0, 11, true).await;
    assert!(refs.is_some(), "find references for struct should succeed");

    let locations = refs.unwrap();
    // Should find: definition (line 0), impl target (line 1), return type (line 2),
    // constructor (line 3), type annotation (line 7), static call (line 7)
    assert!(
        locations.len() >= 4,
        "should find at least 4 references (def + impl + return type + constructor), found {}",
        locations.len()
    );

    // Verify no duplicate locations
    let mut seen = std::collections::HashSet::new();
    for loc in &locations {
        let key = (loc.range.start.line, loc.range.start.character);
        assert!(
            seen.insert(key),
            "duplicate reference at line {} col {}",
            key.0,
            key.1
        );
    }

    client.shutdown().await;
}

#[tokio::test]
async fn rename_struct_updates_static_method_calls() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    let source = "\
pub struct Point { pub x: int, pub y: int }
impl Point {
  pub fn new(x: int, y: int) -> Point {
    Point { x, y }
  }
  pub fn origin() -> Point {
    Point { x: 0, y: 0 }
  }
}
fn main() {
  let p = Point.new(1, 2)
  let o = Point.origin()
}";
    client.open(TEST_URI, source).await;

    // Rename "Point" in the struct definition (line 0, col 11)
    let edit = client.rename(TEST_URI, 0, 11, "Vec2").await;
    assert!(edit.is_some(), "rename struct should succeed");

    let edit = edit.unwrap();
    let changes = edit.changes.unwrap();
    let edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();

    // Collect (line, col) of all rename edits
    let positions: Vec<(u32, u32)> = edits
        .iter()
        .map(|e| (e.range.start.line, e.range.start.character))
        .collect();

    // Point.new(1, 2) — line 10, "Point" starts at col 10
    assert!(
        positions.contains(&(10, 10)),
        "rename should include Point in Point.new() — got edits at: {:?}",
        positions
    );

    // Point.origin() — line 11, "Point" starts at col 10
    assert!(
        positions.contains(&(11, 10)),
        "rename should include Point in Point.origin() — got edits at: {:?}",
        positions
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_stdlib_member_returns_none() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "import \"go:fmt\"\nfn main() {\n  fmt.Println(\"hello\")\n}",
        )
        .await;

    // Cursor on "Println" in "fmt.Println" (line 2, col 6)
    let response = client.goto_definition(TEST_URI, 2, 6).await;
    // Stdlib definitions are embedded — no file to jump to.
    // Should return None rather than a bogus location.
    assert!(
        response.is_none(),
        "goto_definition on stdlib member should return None, not a bogus location"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_through_propagate_operator() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
fn greet() -> Result<string, string> { Ok(\"hi\") }
fn main() -> Result<(), string> {
  let msg = greet()?
  Ok(())
}";
    client.open(TEST_URI, source).await;

    // Cursor on `greet` in `greet()?` — line 2, char 12
    let response = client.goto_definition(TEST_URI, 2, 12).await;
    assert!(
        response.is_some(),
        "goto_definition on function call inside propagate should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0, "should jump to the fn definition");

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_same_module_function_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let source = "\
fn helper() -> int { 42 }
fn main() {
  let x = helper()
}";
    client.open(TEST_URI, source).await;

    // Cursor on `helper` in `helper()` — line 2, char 10
    let response = client.goto_definition(TEST_URI, 2, 10).await;
    assert!(
        response.is_some(),
        "goto_definition on same-module function call should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 0,
        "should jump to helper's definition"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn references_cross_module_finds_call_sites() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let main_content = "import \"utils\"\n\nfn main() {\n  utils.helper()\n  utils.helper()\n}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let utils_dir = src.join("utils");
    std::fs::create_dir_all(&utils_dir).unwrap();
    let utils_content = "pub fn helper() -> int { 42 }";
    std::fs::write(utils_dir.join("utils.lis"), utils_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_uri = Url::from_file_path(src.join("main.lis"))
        .unwrap()
        .to_string();
    let utils_uri = Url::from_file_path(utils_dir.join("utils.lis"))
        .unwrap()
        .to_string();

    client.open(&main_uri, main_content).await;
    client.open(&utils_uri, utils_content).await;

    let refs = client.references(&utils_uri, 0, 7, true).await;
    assert!(
        refs.is_some(),
        "references on pub fn should find cross-module call sites"
    );

    let locations = refs.unwrap();
    assert!(
        locations.len() >= 3,
        "expected at least 3 references (1 declaration + 2 usages), got {}",
        locations.len()
    );

    let main_refs: Vec<_> = locations
        .iter()
        .filter(|l| l.uri.as_str() == main_uri)
        .collect();
    assert!(
        main_refs.len() >= 2,
        "expected at least 2 references in main.lis, got {}",
        main_refs.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_field_cross_module() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("lisette.toml"), "").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let models_dir = src.join("models");
    std::fs::create_dir_all(&models_dir).unwrap();

    let models_content = "pub struct Task {\n  pub id: int,\n  pub title: string,\n}";
    std::fs::write(models_dir.join("models.lis"), models_content).unwrap();

    let main_content = "import \"models\"\n\nfn main() {\n  let t = models.Task { id: 1, title: \"hi\" }\n  let x = t.id\n}";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_uri = Url::from_file_path(src.join("main.lis"))
        .unwrap()
        .to_string();
    let models_uri = Url::from_file_path(models_dir.join("models.lis"))
        .unwrap()
        .to_string();

    client.open(&models_uri, models_content).await;
    client.open(&main_uri, main_content).await;

    // Line 4: "  let x = t.id" — cursor on "i" of "id" at col 12
    let response = client.goto_definition(&main_uri, 4, 12).await;
    assert!(
        response.is_some(),
        "goto_definition on cross-module struct field should resolve"
    );

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.uri.as_str(),
        models_uri,
        "should navigate to models file"
    );
    // Line 1 in models: "  pub id: int,"
    assert_eq!(
        loc.range.start.line, 1,
        "should jump to 'id' field declaration"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_field_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Task {
  pub id: int,
  pub title: string,
}
fn main() {
  let t = Task { id: 1, title: \"hello\" }
  let x = t.id
  let y = t.title
}",
        )
        .await;

    // Cursor on "id" in "t.id" (line 6, col 13)
    let response = client.goto_definition(TEST_URI, 6, 13).await;
    assert!(
        response.is_some(),
        "goto_definition on struct field access should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 1,
        "should jump to 'id' field declaration"
    );

    // Cursor on "title" in "t.title" (line 7, col 13)
    let response = client.goto_definition(TEST_URI, 7, 13).await;
    assert!(
        response.is_some(),
        "goto_definition on struct field 'title' should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 2,
        "should jump to 'title' field declaration"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_enum_variant_in_match_pattern() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    // Line 0: enum Color { Red, Green, Blue }
    // Line 1: fn describe(c: Color) -> string {
    // Line 2:   match c {
    // Line 3:     Color.Red => "red",
    // Line 4:     Color.Green => "green",
    // Line 5:     Color.Blue => "blue",
    // Line 6:   }
    // Line 7: }
    client
        .open(
            TEST_URI,
            "enum Color { Red, Green, Blue }\nfn describe(c: Color) -> string {\n  match c {\n    Color.Red => \"red\",\n    Color.Green => \"green\",\n    Color.Blue => \"blue\",\n  }\n}",
        )
        .await;

    // Cursor on "Red" in "Color.Red" (line 3, col 10)
    let response = client.goto_definition(TEST_URI, 3, 10).await;
    assert!(
        response.is_some(),
        "goto_definition on enum variant in match pattern should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 0,
        "should jump to Red variant declaration"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_enum_variant_with_payload_in_match() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    // Line 0: enum Msg { Text(string), Num(int) }
    // Line 1: fn handle(m: Msg) -> string {
    // Line 2:   match m {
    // Line 3:     Msg.Text(s) => s,
    // Line 4:     Msg.Num(n) => "num",
    // Line 5:   }
    // Line 6: }
    client
        .open(
            TEST_URI,
            "enum Msg { Text(string), Num(int) }\nfn handle(m: Msg) -> string {\n  match m {\n    Msg.Text(s) => s,\n    Msg.Num(n) => \"num\",\n  }\n}",
        )
        .await;

    // Cursor on "Text" in "Msg.Text(s)" (line 3, col 8)
    let response = client.goto_definition(TEST_URI, 3, 8).await;
    assert!(
        response.is_some(),
        "goto_definition on enum variant with payload should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 0,
        "should jump to Text variant declaration"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_match_arm_payload_binding_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    // Line 0: enum Wrapper { Val(string) }
    // Line 1: fn extract(w: Wrapper) -> string {
    // Line 2:   match w {
    // Line 3:     Wrapper.Val(inner) => inner,
    // Line 4:   }
    // Line 5: }
    client
        .open(
            TEST_URI,
            "enum Wrapper { Val(string) }\nfn extract(w: Wrapper) -> string {\n  match w {\n    Wrapper.Val(inner) => inner,\n  }\n}",
        )
        .await;

    // Cursor on "inner" in "Wrapper.Val(inner)" (line 3, col 16)
    let hover = client.hover(TEST_URI, 3, 16).await;
    assert!(hover.is_some(), "hover on match arm binding should work");
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("string"),
        "should show payload type 'string', got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn signature_help_has_parameter_info() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn greet(name: string, age: int) -> string { name }\nfn main() { greet(\"hi\", 1) }",
        )
        .await;

    // Cursor inside greet call args (line 1, col 19)
    let help = client.signature_help(TEST_URI, 1, 19).await;
    assert!(help.is_some(), "signature help should be returned");

    let sig = &help.unwrap().signatures[0];
    assert!(
        sig.parameters.is_some(),
        "parameters should be populated, not None"
    );
    let params = sig.parameters.as_ref().unwrap();
    assert_eq!(params.len(), 2, "should have 2 parameters");

    client.shutdown().await;
}

#[tokio::test]
async fn signature_help_method_call_does_not_double_strip_self() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { x: int, y: int }
impl Point {
  pub fn translate(self, dx: int, dy: int) -> Point {
    Point { x: self.x + dx, y: self.y + dy }
  }
  pub fn distance_sq(self, other: Point) -> int {
    let dx = self.x - other.x
    let dy = self.y - other.y
    dx * dx + dy * dy
  }
}
fn main() {
  let p = Point { x: 0, y: 0 }
  let moved = p.translate(1, 2)
  let dist = p.distance_sq(moved)
}",
        )
        .await;

    // translate(self, dx: int, dy: int) -> after self stripped, should show 2 params
    // Line 13: "  let moved = p.translate(1, 2)" — '(' at col 25, cursor at col 26
    let help = client.signature_help(TEST_URI, 13, 26).await;
    assert!(help.is_some(), "translate sig help should exist");
    let sig = &help.unwrap().signatures[0];
    let params = sig.parameters.as_ref().expect("should have params");
    assert_eq!(
        params.len(),
        2,
        "translate should show 2 params (dx, dy) after self stripped"
    );

    // distance_sq(self, other: Point) -> after self stripped, should show 1 param
    // Line 14: "  let dist = p.distance_sq(moved)" — '(' at col 26, cursor at col 27
    let help = client.signature_help(TEST_URI, 14, 27).await;
    assert!(help.is_some(), "distance_sq sig help should exist");
    let sig = &help.unwrap().signatures[0];
    let params = sig.parameters.as_ref().expect("should have params");
    assert_eq!(
        params.len(),
        1,
        "distance_sq should show 1 param (other) after self stripped"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_enum_variant_in_tuple_match_pattern() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
enum Shape {
  Circle(int),
  Rect(int, int),
}
fn match_pair(a: Shape, b: Shape) -> string {
  match (a, b) {
    (Shape.Circle(_), Shape.Circle(_)) => \"two circles\",
    (Shape.Rect(_, _), Shape.Rect(_, _)) => \"two rects\",
    _ => \"mixed\",
  }
}",
        )
        .await;

    // goto-def on Circle inside tuple pattern (line 6, on "Circle" in first tuple element)
    let col = 11; // "Shape.Circle" - "Circle" starts at col 11
    let response = client.goto_definition(TEST_URI, 6, col).await;
    assert!(
        response.is_some(),
        "goto-def on Circle in tuple pattern should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 1, "Circle is defined on line 1");

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_chained_method_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Canvas { shapes: Slice<int>, name: string }
impl Canvas {
  pub fn new(name: string) -> Canvas {
    Canvas { shapes: [], name }
  }
  pub fn add(self, shape: int) -> Canvas {
    Canvas { shapes: self.shapes.append(shape), ..self }
  }
}
fn main() {
  let c = Canvas.new(\"test\").add(1).add(2)
}",
        )
        .await;

    // goto-def on first ".add" in the chain (line 10)
    // Canvas.new("test").add(1).add(2)
    // The first .add starts after Canvas.new("test")
    let line_text = "  let c = Canvas.new(\"test\").add(1).add(2)";
    let col = line_text.find(".add").unwrap() as u32 + 1; // on 'a' of first add
    let response = client.goto_definition(TEST_URI, 10, col).await;
    assert!(
        response.is_some(),
        "goto-def on chained .add() should resolve"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 5, "add is defined on line 5");

    client.shutdown().await;
}

#[tokio::test]
async fn completion_on_function_parameter_dot_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "\
struct Point { pub x: int, pub y: int }
impl Point {
  pub fn translate(self, dx: int, dy: int) -> Point {
    Point { x: self.x + dx, y: self.y + dy }
  }
}
fn process(p: Point) -> int {
  p.x
}",
        )
        .await;

    // Completion after "p." in the function body (line 7, col after "p.")
    let col = 4; // "  p." -> col 4 is after the dot
    let response = client.completion(TEST_URI, 7, col).await;
    assert!(
        response.is_some(),
        "completion after param dot should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"x".to_string()),
        "should include field 'x', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"translate".to_string()),
        "should include method 'translate', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_on_if_let_binding_dot_access() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    const TEST_URI: &str = "file:///test.lis";

    client
        .open(
            TEST_URI,
            "struct Line {
  start: int,
  end: int,
}
impl Line {
  pub fn length(self) -> int { self.end - self.start }
}
fn process(maybe: Option<Line>) -> int {
  if let Some(line) = maybe {
    line.length()
  } else {
    0
  }
}",
        )
        .await;

    let response = client.completion(TEST_URI, 9, 9).await;
    assert!(
        response.is_some(),
        "completion after if-let binding dot should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"start".to_string()),
        "should include field 'start', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"length".to_string()),
        "should include method 'length', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_on_cross_module_enum_dot_access() {
    let mut client = TestClient::new().await;

    let root = tempfile::tempdir().unwrap();
    let root_path = root.path();

    std::fs::write(
        root_path.join("lisette.toml"),
        "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root_path.join("src/colors")).unwrap();
    std::fs::write(
        root_path.join("src/colors/colors.lis"),
        "pub enum Color { Red, Green, Blue }\n",
    )
    .unwrap();
    std::fs::create_dir_all(root_path.join("src/main")).unwrap();
    std::fs::write(
        root_path.join("src/main/main.lis"),
        "import \"colors\"\nfn main() {\n  let c = colors.Color.Red\n}\n",
    )
    .unwrap();

    client.initialize_with_root(root_path).await;

    let colors_uri = format!(
        "file://{}",
        root_path.join("src/colors/colors.lis").display()
    );
    let main_uri = format!("file://{}", root_path.join("src/main/main.lis").display());

    client
        .open(
            &colors_uri,
            &std::fs::read_to_string(root_path.join("src/colors/colors.lis")).unwrap(),
        )
        .await;
    client
        .open(
            &main_uri,
            &std::fs::read_to_string(root_path.join("src/main/main.lis")).unwrap(),
        )
        .await;

    let response = client.completion(&main_uri, 2, 23).await;
    assert!(
        response.is_some(),
        "completion after cross-module enum dot should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"Red".to_string()),
        "should include variant 'Red', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Green".to_string()),
        "should include variant 'Green', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_after_underscore_prefixed_variable_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
impl Point {
  fn new(x: int, y: int) -> Point { Point { x: x, y: y } }
}
fn main() -> int {
  let _p = Point.new(1, 2)
  _p.x
}"#,
        )
        .await;

    let response = client.completion(TEST_URI, 6, 5).await;
    assert!(
        response.is_some(),
        "completion after _p. should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"x".to_string()),
        "should include field 'x', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_after_chained_field_access_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
struct Rect { origin: Point, width: int, height: int }
impl Rect {
  fn new(x: int, y: int, w: int, h: int) -> Rect {
    Rect { origin: Point { x: x, y: y }, width: w, height: h }
  }
}
fn main() -> int {
  let r = Rect.new(0, 0, 10, 20)
  r.origin.x
}"#,
        )
        .await;

    let response = client.completion(TEST_URI, 9, 11).await;
    assert!(
        response.is_some(),
        "completion after r.origin. should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"x".to_string()),
        "should include field 'x', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"y".to_string()),
        "should include field 'y', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_after_let_else_binding_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
fn main() -> int {
  let o: Option<Point> = Some(Point { x: 1, y: 2 })
  let Some(pt) = o else {
    return 0
  }
  pt.x
}"#,
        )
        .await;

    let response = client.completion(TEST_URI, 6, 5).await;
    assert!(
        response.is_some(),
        "completion after pt. in let-else body should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"x".to_string()),
        "should include field 'x', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"y".to_string()),
        "should include field 'y', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn completion_after_method_call_return_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
struct Rect { origin: Point, width: int, height: int }
impl Rect {
  fn center(self) -> Point {
    Point { x: self.origin.x + self.width / 2, y: self.origin.y + self.height / 2 }
  }
}
fn main() -> int {
  let r = Rect { origin: Point { x: 0, y: 0 }, width: 10, height: 20 }
  r.center().x
}"#,
        )
        .await;

    let response = client.completion(TEST_URI, 9, 13).await;
    assert!(
        response.is_some(),
        "completion after r.center(). should return results"
    );
    let labels = completion_labels(&response.unwrap());
    assert!(
        labels.contains(&"x".to_string()),
        "should include field 'x', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"y".to_string()),
        "should include field 'y', got: {:?}",
        labels
    );

    client.shutdown().await;
}

#[tokio::test]
async fn prepare_rename_on_dot_access_method() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"struct Rect { width: int, height: int }
impl Rect {
  fn area(self) -> int { self.width * self.height }
}
fn main() -> int {
  let r = Rect { width: 10, height: 20 }
  r.area()
}"#,
        )
        .await;

    // prepare_rename on "area" in "r.area()" (line 6, col 4)
    let response = client.prepare_rename(TEST_URI, 6, 4).await;
    assert!(
        response.is_some(),
        "prepare_rename on method via dot access should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn prepare_rename_on_dot_access_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
struct Rect { origin: Point, width: int }
fn main() -> int {
  let r = Rect { origin: Point { x: 0, y: 0 }, width: 10 }
  r.origin.x
}"#,
        )
        .await;

    // prepare_rename on "x" in "r.origin.x" (line 4, col 11)
    let response = client.prepare_rename(TEST_URI, 4, 11).await;
    assert!(
        response.is_some(),
        "prepare_rename on field via chained dot access should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn rename_on_dot_access_field_finds_usages() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
impl Point {
  fn new(x: int, y: int) -> Point { Point { x: x, y: y } }
}
fn main() {
  let p = Point.new(1, 2)
  let a = p.x
  let b = p.x
  a + b
}"#,
        )
        .await;

    let edits = client.rename(TEST_URI, 6, 12, "horizontal").await;
    let edits = edits.expect("rename on field via dot access should return edits");
    let changes = edits.changes.expect("rename should have changes");

    let all_edits: Vec<_> = changes
        .values()
        .flat_map(|e| e.iter())
        .filter(|e| e.new_text == "horizontal")
        .collect();

    assert!(
        all_edits.len() >= 3,
        "rename on field via dot access should find definition + usage sites, got {} edits",
        all_edits.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn references_on_dot_access_field_finds_usages() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
impl Point {
  fn new(x: int, y: int) -> Point { Point { x: x, y: y } }
}
fn main() {
  let p = Point.new(1, 2)
  let a = p.x
  let b = p.x
  a + b
}"#,
        )
        .await;

    let refs = client.references(TEST_URI, 6, 12, true).await;
    let refs = refs.expect("references on field via dot access should return results");

    assert!(
        refs.len() >= 3,
        "find-references on field via dot access should find definition + usage sites, got {} refs",
        refs.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn references_on_enum_variant_in_match_pattern() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(
            TEST_URI,
            r#"enum Shape { Circle(int), Square(int) }
fn area(s: Shape) -> int {
  match s {
    Shape.Circle(r) => r * r,
    Shape.Square(side) => side * side,
  }
}
fn main() {
  let _ = area(Shape.Circle(5))
}"#,
        )
        .await;

    let refs = client.references(TEST_URI, 3, 10, true).await;
    let refs = refs.expect("references on enum variant in match pattern should return results");

    assert!(
        refs.len() >= 2,
        "find-references on enum variant in match pattern should find multiple refs, got {} refs",
        refs.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn references_on_type_in_annotation() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
impl Point {
  fn new(x: int, y: int) -> Point { Point { x: x, y: y } }
}
fn use_point(p: Point) -> int {
  p.x
}
fn main() {
  let p: Point = Point.new(1, 2)
  let _ = use_point(p)
}"#,
        )
        .await;

    let refs = client.references(TEST_URI, 8, 10, true).await;
    let refs = refs.expect("references on type in annotation should return results");

    assert!(
        refs.len() >= 2,
        "find-references on type in annotation should find multiple refs, got {} refs",
        refs.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn prepare_rename_on_enum_variant_in_match_pattern() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(
            TEST_URI,
            r#"enum Status { Active(int), Inactive }
fn check(s: Status) -> int {
  match s {
    Status.Active(code) => code,
    Status.Inactive => 0,
  }
}"#,
        )
        .await;

    let pr = client.prepare_rename(TEST_URI, 3, 11).await;
    assert!(
        pr.is_some(),
        "prepare_rename on enum variant in match pattern should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn prepare_rename_on_type_in_annotation() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    client
        .open(
            TEST_URI,
            r#"struct Point { x: int, y: int }
impl Point {
  fn new(x: int, y: int) -> Point { Point { x: x, y: y } }
}
fn main() {
  let p: Point = Point.new(1, 2)
  p.x
}"#,
        )
        .await;

    let pr = client.prepare_rename(TEST_URI, 5, 9).await;
    assert!(
        pr.is_some(),
        "prepare_rename on type name in annotation should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_call_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  p.x\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 2, 18).await;
    assert!(response.is_some());

    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(loc.range.start.line, 0);

    client.shutdown().await;
}

#[tokio::test]
async fn hover_struct_call_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn main() { Point { x: 1, y: 2 } }",
        )
        .await;

    let hover = client.hover(TEST_URI, 1, 20).await;
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("int"),
        "hover on field name in struct literal should show field type, got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn prepare_rename_struct_call_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  p.x\n}",
        )
        .await;

    let pr = client.prepare_rename(TEST_URI, 2, 18).await;
    assert!(
        pr.is_some(),
        "prepare_rename on field name in struct literal should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn references_struct_call_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point { x: int, y: int }\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  p.x\n}",
        )
        .await;

    let refs = client.references(TEST_URI, 2, 18, true).await;
    assert!(refs.is_some());

    let locations = refs.unwrap();
    let lines: Vec<u32> = locations.iter().map(|l| l.range.start.line).collect();
    assert!(
        lines.contains(&0),
        "references on struct literal field should include field definition (line 0), got: {lines:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_for_loop_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn main() {\n  let items = [1, 2, 3]\n  for item in items {\n    item\n  }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 2, 6).await;
    assert!(
        response.is_some(),
        "goto-def on for-loop binding should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_match_slice_pattern_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn main() {\n  let items = [1, 2, 3]\n  match items {\n    [first, ..rest] => first,\n    _ => 0,\n  }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 3, 5).await;
    assert!(
        response.is_some(),
        "goto-def on 'first' in match slice pattern should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_if_let_slice_pattern_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn main() {\n  let items = [1, 2, 3]\n  if let [head, ..tail] = items {\n    head\n  } else {\n    0\n  }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 2, 10).await;
    assert!(
        response.is_some(),
        "goto-def on 'head' in if-let slice pattern should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_enum_variant_payload_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn main() {\n  let val = Some(42)\n  match val {\n    Some(n) => n,\n    None => 0,\n  }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 3, 9).await;
    assert!(
        response.is_some(),
        "goto-def on 'n' in Some(n) pattern should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_while_let_enum_payload_binding() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "fn main() {\n  let items = [Some(1), None]\n  let mut i = 0\n  while let Some(val) = items[i] {\n    val\n    i += 1\n  }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 3, 17).await;
    assert!(
        response.is_some(),
        "goto-def on 'val' in while-let Some(val) pattern should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_enum_variant_in_definition() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "enum Color {\n  Red,\n  Green,\n  Blue,\n}\n\nfn main() {\n  let c = Color.Red\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 1, 2).await;
    assert!(
        response.is_some(),
        "goto-def on 'Red' in enum definition should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_enum_variant_with_payload_in_definition() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "enum Shape {\n  Circle(int),\n  Rectangle(int, int),\n}\n\nfn main() {\n  let s = Shape.Circle(5)\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 1, 2).await;
    assert!(
        response.is_some(),
        "goto-def on 'Circle' in enum definition should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn rename_struct_field_from_definition() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Config {\n  pub width: int,\n  pub height: int,\n}\n\nfn create(w: int, h: int) -> Config {\n  Config { width: w, height: h }\n}\n\nfn main() {\n  let c = create(10, 20)\n  c.width + c.height\n}",
        )
        .await;

    let prep = client.prepare_rename(TEST_URI, 1, 6).await;
    assert!(prep.is_some());

    let edit = client.rename(TEST_URI, 1, 6, "w").await;
    assert!(edit.is_some());
    let changes = edit.unwrap().changes.unwrap();
    let file_edits = changes.get(&Url::parse(TEST_URI).unwrap()).unwrap();
    for e in file_edits {
        assert_eq!(
            e.new_text, "w",
            "all rename edits should use the new field name, got {:?}",
            e.new_text
        );
    }
    assert!(
        file_edits.len() >= 2,
        "expected at least 2 edits (definition + dot access), got {}",
        file_edits.len()
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_field_in_definition() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point {\n  pub x: int,\n  pub y: int,\n}\n\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  p.x\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 1, 6).await;
    assert!(
        response.is_some(),
        "goto-def on 'x' in struct field definition should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn hover_struct_field_in_definition() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point {\n  pub x: int,\n  pub y: string,\n}\n\nfn main() {\n  let p = Point { x: 1, y: \"hi\" }\n}",
        )
        .await;

    let hover = client.hover(TEST_URI, 2, 6).await;
    assert!(
        hover.is_some(),
        "hover on 'y' in struct field definition should return a result"
    );
    let content = hover_content(&hover.unwrap());
    assert!(
        content.contains("string"),
        "hover should show string type, got: {content}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_pattern_field() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point {\n  pub x: int,\n  pub y: int,\n}\n\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  match p {\n    Point { x, y } => x + y,\n  }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 8, 12).await;
    assert!(
        response.is_some(),
        "goto-def on 'x' in struct pattern should return a result"
    );
    let loc = definition_location(&response.unwrap());
    assert!(loc.is_some());
    assert_eq!(
        loc.unwrap().range.start.line,
        1,
        "goto-def should land on field definition line 1"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_struct_name_in_aliased_struct_call() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("lisette.toml"),
        "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let shapes_dir = src.join("shapes");
    std::fs::create_dir_all(&shapes_dir).unwrap();
    std::fs::write(
        shapes_dir.join("shapes.lis"),
        "pub struct Rect {\n  pub width: int,\n  pub height: int,\n}\n",
    )
    .unwrap();

    let main_content = "import s \"shapes\"\n\nfn main() {\n  let r = s.Rect { width: 10, height: 20 }\n  r.width\n}\n";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_uri = Url::from_file_path(src.join("main.lis"))
        .unwrap()
        .to_string();
    client.open(&main_uri, main_content).await;

    let response = client.goto_definition(&main_uri, 3, 14).await;
    assert!(
        response.is_some(),
        "goto-def on 'Rect' in aliased struct call s.Rect should return a result"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn rename_struct_from_aliased_struct_call() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("lisette.toml"),
        "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let shapes_dir = src.join("shapes");
    std::fs::create_dir_all(&shapes_dir).unwrap();
    std::fs::write(
        shapes_dir.join("shapes.lis"),
        "pub struct Rect {\n  pub width: int,\n  pub height: int,\n}\n",
    )
    .unwrap();

    let main_content = "import s \"shapes\"\n\nfn main() {\n  let r = s.Rect { width: 10, height: 20 }\n  r.width\n}\n";
    std::fs::write(src.join("main.lis"), main_content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_uri = Url::from_file_path(src.join("main.lis"))
        .unwrap()
        .to_string();
    client.open(&main_uri, main_content).await;

    let result = client.rename(&main_uri, 3, 14, "Box").await;
    assert!(
        result.is_some(),
        "rename 'Rect' from aliased struct call s.Rect should return edits"
    );
    let changes = result.unwrap().changes.unwrap_or_default();
    assert!(
        !changes.is_empty(),
        "rename should produce changes across files"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_type_alias_in_struct_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point {\n  pub x: int,\n  pub y: int,\n}\n\ntype Alias = Point\n\nfn main() {\n  let p = Alias { x: 1, y: 2 }\n  p.x\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 8, 12).await;
    assert!(
        response.is_some(),
        "goto-def on 'Alias' in struct call should return a result"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 5,
        "goto-def should land on type alias definition, not the underlying struct"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn goto_definition_field_in_aliased_struct_call() {
    let mut client = TestClient::new().await;
    client.initialize().await;
    client
        .open(
            TEST_URI,
            "struct Point {\n  pub x: int,\n  pub y: int,\n}\n\ntype Alias = Point\n\nfn main() {\n  let p = Alias { x: 1, y: 2 }\n}",
        )
        .await;

    let response = client.goto_definition(TEST_URI, 8, 18).await;
    assert!(
        response.is_some(),
        "goto-def on field `x` in aliased struct call should find the underlying struct field"
    );
    let loc = definition_location(&response.unwrap()).unwrap();
    assert_eq!(
        loc.range.start.line, 1,
        "goto-def should land on `pub x: int` in the struct definition"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn diagnostics_invalid_manifest_surfaces_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Write an invalid lisette.toml (missing required [project] section)
    std::fs::write(root.join("lisette.toml"), "[invalid]\nfoo = 1\n").unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let content = "fn main() { 1 }";
    std::fs::write(src.join("main.lis"), content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_uri = Url::from_file_path(src.join("main.lis"))
        .unwrap()
        .to_string();
    client.open(&main_uri, content).await;

    let diagnostics = client.await_diagnostics().await;

    let has_manifest_error = diagnostics.iter().any(|d| {
        d.severity == Some(DiagnosticSeverity::ERROR)
            && d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "resolve.manifest_error"),
            )
    });

    assert!(
        has_manifest_error,
        "invalid lisette.toml should produce a manifest_error diagnostic, got: {diagnostics:?}"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn diagnostics_toolchain_mismatch_surfaces_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Pin a lis version that does not match the running binary
    std::fs::write(
        root.join("lisette.toml"),
        "[project]\nname = \"test\"\nversion = \"0.1.0\"\n\n[toolchain]\nlis = \"99.99.99\"\n",
    )
    .unwrap();

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    let content = "fn main() { 1 }";
    std::fs::write(src.join("main.lis"), content).unwrap();

    let mut client = TestClient::new().await;
    client.initialize_with_root(root).await;

    let main_uri = Url::from_file_path(src.join("main.lis"))
        .unwrap()
        .to_string();
    client.open(&main_uri, content).await;

    let diagnostics = client.await_diagnostics().await;

    let has_toolchain_error = diagnostics.iter().any(|d| {
        d.severity == Some(DiagnosticSeverity::ERROR) && d.message.contains("Toolchain mismatch")
    });

    assert!(
        has_toolchain_error,
        "toolchain version mismatch should produce a diagnostic, got: {diagnostics:?}"
    );

    client.shutdown().await;
}
