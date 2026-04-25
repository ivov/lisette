use super::Parser;
use crate::ast::{Expression, Literal};
use crate::lex::TokenKind::*;
use crate::types::Type;

impl<'source> Parser<'source> {
    pub fn parse_directive(&mut self) -> Expression {
        let start = self.current_token();
        let directive_name = start.text.strip_prefix('@').unwrap_or(start.text);

        match directive_name {
            "rawgo" => self.parse_rawgo_directive(),
            _ => {
                self.track_error("unknown directive", "There is no such directive.");
                self.next();
                if self.is(LeftParen) {
                    self.collect_delimited_expressions(LeftParen, RightParen);
                }
                Expression::Unit {
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                }
            }
        }
    }

    fn parse_rawgo_directive(&mut self) -> Expression {
        self.ensure(Directive);

        let (args, _) = self.collect_delimited_expressions(LeftParen, RightParen);

        let text = match args.into_iter().next() {
            Some(Expression::Literal {
                literal: Literal::String { value, .. },
                ..
            }) => value,

            _ => {
                self.track_error("invalid call to rawgo", "Use `@rawgo(\"go code\")`.");
                "?".to_string()
            }
        };

        Expression::RawGo { text }
    }
}
