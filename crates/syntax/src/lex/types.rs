use crate::lex::Token;
use crate::parse::ParseError;

#[derive(Debug)]
pub struct LexResult<'source> {
    pub tokens: Vec<Token<'source>>,
    pub errors: Vec<ParseError>,
    pub trivia: Trivia,
}

impl<'source> LexResult<'source> {
    pub fn failed(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Debug, Default, Clone)]
pub struct Trivia {
    pub comments: Vec<(u32, u32)>,
    pub doc_comments: Vec<(u32, u32)>,
    pub blank_lines: Vec<u32>,
}
