pub mod ast;
pub mod attributes;
pub mod containment;
mod display;
pub mod go_names;
pub mod lex;
pub mod parse;
pub mod program;
pub mod types;

pub use ecow::EcoString;
pub use parse::ParseError;

pub const ENTRY_MODULE_ID: &str = "_entry_";
pub const ROOT_IMPORT: &str = "root";

pub type AstBuildResult = parse::ParseResult;

#[cfg(target_pointer_width = "64")]
mod size_assertions {
    use std::mem::size_of;
    const _: () = assert!(size_of::<super::ast::Expression>() == 288);
    const _: () = assert!(size_of::<super::ast::Pattern>() == 152);
    const _: () = assert!(size_of::<super::types::Type>() == 40);
    const _: () = assert!(size_of::<super::ast::Span>() == 12);
}

const MAX_SOURCE_BYTES: usize = 10 * 1024 * 1024; // 10 MiB

pub fn build_ast(source: &str, file_id: u32) -> AstBuildResult {
    if source.len() > MAX_SOURCE_BYTES {
        return parse::ParseResult {
            ast: vec![],
            errors: vec![
                ParseError::new(
                    "File too large",
                    ast::Span::new(file_id, 0, 0),
                    format!(
                        "file is {} bytes, maximum is {} bytes",
                        source.len(),
                        MAX_SOURCE_BYTES,
                    ),
                )
                .with_parse_code("file_too_large"),
            ],
            file_comment: None,
        };
    }

    let parse_result = parse::Parser::lex_and_parse_file(source, file_id);
    if parse_result.failed() {
        return parse::ParseResult {
            ast: vec![],
            errors: parse_result.errors,
            file_comment: None,
        };
    }

    parse_result
}

#[cfg(test)]
mod tests {
    use super::ast::{BinaryOperator, Expression};

    fn contains_pipeline(expression: &Expression) -> bool {
        matches!(
            expression,
            Expression::Binary {
                operator: BinaryOperator::Pipeline,
                ..
            }
        ) || expression.children().into_iter().any(contains_pipeline)
    }

    #[test]
    fn build_ast_preserves_pipeline_expression() {
        let result = super::build_ast("fn test() { value |> transform }", 0);

        assert!(result.errors.is_empty());
        assert!(result.ast.iter().any(contains_pipeline));
    }
}
