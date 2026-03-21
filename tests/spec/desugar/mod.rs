use crate::assert_desugar_snapshot;

#[test]
fn pipeline_simple() {
    let input = "fn test() { x |> func; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_chained() {
    let input = "fn test() { x |> f |> g; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_triple_chained() {
    let input = "fn test() { x |> f |> g |> h; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_with_partial_application() {
    let input = "fn test() { x |> add(5); }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_chained_with_partial_application() {
    let input = "fn test() { x |> add(5) |> multiply(2); }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_partial_application_multiple_args() {
    let input = "fn test() { x |> clamp(0, 100); }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_with_arithmetic() {
    let input = "fn test() { (1 + 2) |> double; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_preserves_other_operators() {
    let input = "fn test() { let a = 1 + 2; let b = a |> double; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_in_block() {
    let input = r#"
fn test() {
  {
    let x = 5;
    x |> double
  }
}
"#;
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_in_let() {
    let input = "fn test() { let result = x |> func; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_as_return() {
    let input = "fn test() { x |> double; }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_nested_calls() {
    let input = "fn test() { x |> add(multiply(2, 3)) |> subtract(1); }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_multiple_in_function() {
    let input = r#"
fn test() {
  let a = x |> double;
  let b = y |> triple;
  a + b
}
"#;
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_with_parens() {
    let input = "fn test() { x |> (f); }";
    assert_desugar_snapshot!(input);
}

#[test]
fn pipeline_in_format_string() {
    let input = r#"fn test() { let x = 5; f"result: {x |> double}"; }"#;
    assert_desugar_snapshot!(input);
}
