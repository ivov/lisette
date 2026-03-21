mod comments;
mod formatter;
mod lindig;

pub use formatter::Formatter;

use comments::Comments;
use syntax::ParseError;
use syntax::lex::Lexer;
use syntax::parse::Parser;

const MAX_LINE_WIDTH: isize = 80;
const INDENT_WIDTH: isize = 2;

pub fn format_source(source: &str) -> Result<String, Vec<ParseError>> {
    let lex_result = Lexer::new(source, 0).lex();
    if lex_result.failed() {
        return Err(lex_result.errors);
    }

    let parse_result = Parser::new(lex_result.tokens, source).parse();
    if parse_result.failed() {
        return Err(parse_result.errors);
    }

    let comments = Comments::from_trivia(&lex_result.trivia, source);
    let mut formatter = Formatter::new(comments);
    let document = formatter.module(&parse_result.ast);
    let output = document.to_pretty_string(MAX_LINE_WIDTH);

    Ok(output)
}
