use super::Parser;
use crate::ast::Expression;
use crate::{build_ast, build_ast_recovering};

fn body_items(ast: &[Expression]) -> &[Expression] {
    let Expression::Function { body, .. } = &ast[0] else {
        panic!("expected a function");
    };
    let Expression::Block { items, .. } = body.definition().unwrap() else {
        panic!("expected a block");
    };
    items
}

#[test]
fn missing_initializer_is_reported_at_equals() {
    for following in ["let other = 2", "const OTHER = 2", "fn other() {}", "", ";"] {
        let source = format!("fn main() {{\n  let total: int =\n  {following}\n}}");
        let result = build_ast(&source, 0);
        assert_eq!(result.errors.len(), 1, "{source}: {:?}", result.errors);
        let error = &result.errors[0];
        assert_eq!(error.code, "parse.expected_expression");
        let span = error.labels[0].0;
        assert_eq!(span.byte_offset as usize, source.find('=').unwrap());
        assert_eq!(span.byte_length, 1);
    }
}

#[test]
fn initializer_recovery_preserves_statement_starts() {
    for statement in [
        "let other = 2",
        "while false {}",
        "for n in [] {}",
        "loop { break }",
        "if true {}",
        "match value { _ => 0 }",
        "return 0",
        "break",
        "continue",
        "defer finish()",
        "assert true",
        "task finish()",
        "try {}",
        "recover {}",
        "select { _ => 0 }",
        "fn other() {}",
        "const OTHER = 2",
    ] {
        let source = format!("fn main() {{\n  let bad = @\n  {statement}\n  let after = 3\n}}");
        let result = build_ast_recovering(&source, 0);
        assert_eq!(result.errors.len(), 1, "{statement}: {:?}", result.errors);
        let items = body_items(&result.ast);
        assert_eq!(items.len(), 3, "{statement}: {items:?}");
        assert_eq!(
            items[1].get_span().byte_offset as usize,
            source.find(statement).unwrap(),
            "{statement}"
        );
        assert!(matches!(&items[2], Expression::Let { binding, .. }
            if binding.pattern.get_identifier().as_deref() == Some("after")));
    }
}

#[test]
fn recovery_leaves_delimiters_for_the_enclosing_expression() {
    for initializer in ["take(, 2)", "(1 + )", "[1 + ]", "take(1 + , 2)"] {
        let source = format!("fn main() {{ let bad = {initializer}; let after = 3 }}");
        let result = build_ast_recovering(&source, 0);
        assert_eq!(result.errors.len(), 1, "{initializer}: {:?}", result.errors);
        assert_eq!(body_items(&result.ast).len(), 2, "{initializer}");
    }
}

#[test]
fn rejected_keywords_in_lists_do_not_prevent_progress() {
    for expression in [
        "take(let, 2)",
        "[const, 2]",
        "(struct, 2)",
        "take(fn(x: int) {})",
        "[fn() {}]",
    ] {
        let source = format!("fn main() {{ let bad = {expression} }}\nfn after() {{}}");
        let result = Parser::lex_and_parse_file(&source, 0);
        assert!(!result.errors.is_empty(), "{expression}");
        assert!(!result.truncated, "{expression}: {:?}", result.errors);
        assert!(
            matches!(result.ast.last(), Some(Expression::Function { name, .. }) if name == "after"),
            "{expression}: {:?}; {:?}",
            result.ast,
            result.errors
        );
    }
}

#[test]
fn valid_multiline_initializers_are_still_expressions() {
    for initializer in [
        "if true { 1 } else { 2 }",
        "match value { _ => 1 }",
        "loop { break 1 }",
        "try { 1 }",
        "recover { 1 }",
        "select { _ => 1 }",
        "1 +\n  2",
        "(1,\n  2)",
        "|| 1",
    ] {
        let source = format!("fn main() {{ let value =\n  {initializer}\n}}");
        let result = build_ast(&source, 0);
        assert!(
            result.errors.is_empty(),
            "{initializer}: {:?}",
            result.errors
        );
    }
}
