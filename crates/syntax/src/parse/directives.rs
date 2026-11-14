use super::Parser;
use crate::ast::{Expression, Literal};
use crate::lex::TokenKind::*;

impl<'source> Parser<'source> {
    pub(crate) fn parse_directive(&mut self) -> Expression {
        let start = self.current_token();
        let directive_name = start.text.strip_prefix('@').unwrap_or(start.text);

        match directive_name {
            "rawgo" => self.parse_rawgo_directive(),
            _ => {
                self.track_coded_error(
                    "Unknown directive",
                    "unknown_directive",
                    "no such directive",
                    "`@rawgo` is the only directive",
                );
                self.next();
                if self.is(LeftParen) {
                    self.collect_delimited_expressions(LeftParen, RightParen);
                }
                Self::error_expression(self.span_from_offset(start.byte_offset))
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
                self.track_coded_error(
                    "Invalid `@rawgo` call",
                    "invalid_rawgo",
                    "invalid call to rawgo",
                    "Use `@rawgo(\"go code\")`.",
                );
                "?".to_string()
            }
        };

        Expression::RawGo { text }
    }
}
