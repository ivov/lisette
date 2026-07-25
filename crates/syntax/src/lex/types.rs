use crate::lex::Token;
use crate::parse::ParseError;

#[derive(Debug)]
pub struct LexResult<'source> {
    pub tokens: Vec<Token<'source>>,
    pub errors: Vec<ParseError>,
    pub blank_lines: Vec<u32>,
}

impl<'source> LexResult<'source> {
    pub fn failed(&self) -> bool {
        !self.errors.is_empty()
    }
}
