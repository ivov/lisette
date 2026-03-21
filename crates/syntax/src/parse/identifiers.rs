use ecow::EcoString;

use super::Parser;
use crate::lex::TokenKind::*;

impl<'source> Parser<'source> {
    pub fn read_identifier(&mut self) -> EcoString {
        let identifier = self.current_token().text;

        self.ensure(Identifier);

        identifier.into()
    }

    pub fn read_identifier_sequence(&mut self) -> EcoString {
        let mut name = self.current_token().text.to_string();
        self.ensure(Identifier);

        while self.advance_if(Dot) {
            name.push('.');
            name.push_str(self.current_token().text);
            self.ensure(Identifier);
        }

        name.into()
    }
}
