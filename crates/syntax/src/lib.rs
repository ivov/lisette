pub mod ast;
pub mod attributes;
pub mod containment;
pub mod dependency_block;
mod display;
pub mod go_names;
pub mod go_platform;
pub mod imports;
pub mod lex;
pub mod parse;
pub mod program;
pub mod types;

pub use ecow::EcoString;
pub use parse::{FileParseStatus, ParseError};

pub const ENTRY_PACKAGE_ID: &str = "_entry_";
pub const ROOT_IMPORT: &str = "root";

pub type AstBuildResult = parse::ParseResult;

#[cfg(target_pointer_width = "64")]
mod size_assertions {
    use std::mem::size_of;
    const _: () = assert!(size_of::<super::ast::Expression>() == 304);
    const _: () = assert!(size_of::<super::ast::Pattern>() == 160);
    const _: () = assert!(size_of::<super::types::Type>() == 48);
    const _: () = assert!(size_of::<super::ast::Span>() == 12);
}

const MAX_SOURCE_BYTES: usize = 10 * 1024 * 1024; // 10 MiB

pub fn build_ast(source: &str, file_id: u32) -> AstBuildResult {
    let parse_result = match oversize(source, file_id) {
        Some(result) => return result,
        None => parse::Parser::lex_and_parse_file(source, file_id),
    };
    if parse_result.has_errors() {
        return parse::ParseResult {
            ast: vec![],
            errors: parse_result.errors,
            file_comment: None,
            truncated: true,
            status: FileParseStatus::Failed,
        };
    }

    parse_result
}

pub fn build_ast_recovering(source: &str, file_id: u32) -> AstBuildResult {
    match oversize(source, file_id) {
        Some(result) => result,
        None => parse::Parser::lex_and_parse_file(source, file_id),
    }
}

fn oversize(source: &str, file_id: u32) -> Option<AstBuildResult> {
    (source.len() > MAX_SOURCE_BYTES).then(|| parse::ParseResult {
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
        truncated: true,
        status: FileParseStatus::Failed,
    })
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
    fn a_parser_error_before_any_item_is_recovered_not_failed() {
        let result = super::build_ast_recovering(")", 0);

        assert!(!result.errors.is_empty() && result.ast.is_empty());
        assert_eq!(result.status, super::FileParseStatus::Recovered);
    }

    #[test]
    fn a_lexer_failure_is_the_only_recovering_failure() {
        let result = super::build_ast_recovering("fn main() { \"unterminated }", 0);

        assert_eq!(result.status, super::FileParseStatus::Failed);
    }

    #[test]
    fn a_strict_parse_error_discards_the_ast() {
        let result = super::build_ast("fn valid() {}\nfn broken(", 0);

        assert!(result.ast.is_empty());
        assert_eq!(result.status, super::FileParseStatus::Failed);
    }

    #[test]
    fn build_ast_preserves_pipeline_expression() {
        let result = super::build_ast("fn test() { value |> transform }", 0);

        assert!(result.errors.is_empty());
        assert!(result.ast.iter().any(contains_pipeline));
    }

    #[test]
    fn a_shebang_leaves_the_items_and_their_offsets_alone() {
        let body = "import \"go:fmt\"\n\nfn main() {}\n";
        let with_shebang = format!("#!/usr/bin/env -S lis run\n\n{body}");
        let shift = with_shebang.len() - body.len();

        let bare = super::build_ast(body, 0);
        let result = super::build_ast(&with_shebang, 0);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let offsets = |items: &[Expression]| {
            items
                .iter()
                .map(|item| item.get_span().byte_offset as usize)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            offsets(&result.ast),
            offsets(&bare.ast)
                .into_iter()
                .map(|offset| offset + shift)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_file_comment_may_open_what_the_shebang_leaves() {
        for source in [
            "#!/usr/bin/env -S lis run\n//! A tool.\nfn main() {}",
            "#!/usr/bin/env -S lis run\n\n//! A tool.\nfn main() {}",
        ] {
            let result = super::build_ast(source, 0);

            assert!(result.errors.is_empty(), "{source:?}: {:?}", result.errors);
            assert_eq!(
                result.file_comment.as_deref(),
                Some("A tool."),
                "{source:?}"
            );
        }
    }

    #[test]
    fn a_file_comment_detached_from_the_shebang_is_misplaced() {
        for source in [
            "#!/usr/bin/env -S lis run\n\n\n//! A tool.\nfn main() {}",
            "#!/usr/bin/env -S lis run\r\n\r\n\r\n//! A tool.\r\nfn main() {}",
        ] {
            let result = super::build_ast(source, 0);

            assert_eq!(
                result
                    .errors
                    .iter()
                    .map(|error| error.code.as_str())
                    .collect::<Vec<_>>(),
                ["parse.misplaced_file_comment"],
                "{source:?}"
            );
        }
    }

    #[test]
    fn a_file_comment_below_an_item_stays_misplaced_under_a_shebang() {
        let result = super::build_ast("#!/usr/bin/env -S lis run\nfn main() {}\n//! Late.\n", 0);

        assert_eq!(
            result
                .errors
                .iter()
                .map(|error| error.code.as_str())
                .collect::<Vec<_>>(),
            ["parse.misplaced_file_comment"]
        );
    }
}
