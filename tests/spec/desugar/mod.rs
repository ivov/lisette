use crate::assert_desugar_snapshot;

#[test]
fn desugar_only_rewrites_pipelines() {
    let source = r#"
fn main() {
  let a = if let Some(v) = opt { v } else { 0 }
  let b = match n { 1 => "a", _ => "b" }
  let c = Point { x: 1, y: 2 }
  let d = !!flag
  let e = first + second * third - 0
  let g = obj.method(arg).field
  let h = f"hello {name} world"
  for item in items { print(item) }
  while cond { break }
}
"#;

    let lexed = syntax::lex::Lexer::new(source, 0).lex();
    assert!(!lexed.failed(), "lex failed: {:?}", lexed.errors);
    let parsed = syntax::parse::Parser::new(lexed.tokens, source).parse();
    assert!(!parsed.failed(), "parse failed: {:?}", parsed.errors);

    let before = parsed.ast.clone();
    let desugared = syntax::desugar::desugar(parsed.ast);
    assert!(
        desugared.errors.is_empty(),
        "desugar errors: {:?}",
        desugared.errors
    );
    assert_eq!(
        desugared.ast, before,
        "desugar rewrote a non-pipeline construct, breaking the lint-autofix source-provenance contract"
    );
}

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
