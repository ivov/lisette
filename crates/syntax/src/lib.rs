pub mod ast;
pub(crate) mod ast_folder;
pub mod attributes;
pub mod containment;
pub mod desugar;
mod display;
pub mod go_names;
pub mod lex;
pub mod parse;
pub mod program;
pub mod types;

pub use ecow::EcoString;
pub use parse::ParseError;

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

    let desugar_result = desugar::desugar(parse_result.ast);
    parse::ParseResult {
        ast: desugar_result.ast,
        errors: desugar_result.errors,
        file_comment: parse_result.file_comment,
    }
}
