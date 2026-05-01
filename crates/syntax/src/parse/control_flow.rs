use super::Parser;
use crate::ast::{Expression, MatchArm, MatchOrigin, Span};
use crate::lex::TokenKind::*;
use crate::types::Type;

impl<'source> Parser<'source> {
    pub(super) fn parse_match(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Match);

        let subject = self.with_control_flow_header(|p| p.parse_expression());

        self.ensure(LeftCurlyBrace);

        let mut arms = vec![];

        while self.is_not(RightCurlyBrace) {
            let start_position = self.stream.position;
            let arm = self.parse_match_arm();
            let block_bodied = arm.as_ref().is_some_and(|a| a.expression.is_block());
            if let Some(arm) = arm {
                arms.push(arm);
            }

            if block_bodied && !self.at_match_arm_terminator() {
                let span = self.span_from_token(self.previous_token);
                self.error_match_arm_missing_comma(span);
                self.recover_to_comma_or(RightCurlyBrace);
            } else {
                self.expect_comma_or(RightCurlyBrace);
            }

            self.ensure_progress(start_position, RightCurlyBrace);
        }

        self.ensure(RightCurlyBrace);

        Expression::Match {
            ty: Type::uninferred(),
            subject: subject.into(),
            arms,
            origin: MatchOrigin::Explicit,
            span: self.span_from_tokens(start),
        }
    }

    fn parse_match_arm(&mut self) -> Option<MatchArm> {
        if self.is(Imaginary) {
            self.track_error(
                "not allowed",
                "Imaginary literals are not supported in patterns",
            );
            self.next();
            return None;
        }

        if !self.can_start_pattern() {
            self.track_error("expected pattern", "Match arms must start with a pattern.");
            return None;
        }

        let pattern = self.parse_pattern_allowing_or();

        let guard = if self.advance_if(If) {
            Some(Box::new(self.parse_expression()))
        } else {
            None
        };

        self.ensure(ArrowDouble);

        Some(MatchArm {
            pattern,
            guard,
            typed_pattern: None,
            expression: Box::new(self.parse_assignment()),
        })
    }

    pub(super) fn parse_if(&mut self) -> Expression {
        let start = self.current_token();

        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            self.resync_on_error();
            return Expression::Unit {
                ty: Type::uninferred(),
                span,
            };
        }

        self.ensure(If);

        if self.is(Let) {
            let result = self.parse_if_let_expression(start);
            self.leave_recursion();
            return result;
        }

        let condition = self.with_control_flow_header(|p| p.parse_expression());
        let consequence = self.parse_block_expression();

        let alternative = if self.advance_if(Else) {
            if self.is(If) {
                self.parse_if()
            } else {
                self.parse_block_expression()
            }
        } else {
            Expression::Unit {
                ty: Type::uninferred(),
                span: self.span_from_tokens(start),
            }
        };

        self.leave_recursion();

        Expression::If {
            ty: Type::uninferred(),
            condition: condition.into(),
            consequence: consequence.into(),
            alternative: alternative.into(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_if_let_expression(&mut self, start: crate::lex::Token) -> Expression {
        self.ensure(Let);

        let pattern = self.parse_pattern_allowing_or();
        self.ensure(Equal);
        let scrutinee = self.with_control_flow_header(|p| p.parse_expression());
        let consequence = self.parse_block_expression();

        let (alternative, else_span) = if self.is(Else) {
            let else_token = self.current_token();
            let else_span = Span::new(self.file_id, else_token.byte_offset, else_token.byte_length);
            self.next(); // consume `else`
            let alt = if self.is(If) {
                self.parse_if()
            } else {
                self.parse_block_expression()
            };
            (alt, Some(else_span))
        } else {
            (
                Expression::Unit {
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                },
                None,
            )
        };

        Expression::IfLet {
            pattern,
            scrutinee: scrutinee.into(),
            consequence: consequence.into(),
            alternative: alternative.into(),
            typed_pattern: None,
            else_span,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub(super) fn parse_return(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Return);

        let expression = match self.current_token().kind {
            Semicolon | RightCurlyBrace => Expression::Unit {
                ty: Type::uninferred(),
                span: self.span_from_tokens(start),
            },
            _ => self.parse_expression(),
        };

        Expression::Return {
            expression: expression.into(),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub(super) fn parse_for(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(For);

        let binding = self.parse_binding();

        self.ensure(In_);

        let iterable = self.with_control_flow_header(|p| p.parse_expression());
        let body = self.parse_block_expression();

        Expression::For {
            binding: Box::new(binding),
            iterable: iterable.into(),
            body: body.into(),
            span: self.span_from_tokens(start),
            needs_label: false,
        }
    }

    pub(super) fn parse_while(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(While);

        if self.is(Let) {
            return self.parse_while_let(start);
        }

        let condition = self.with_control_flow_header(|p| p.parse_expression());
        let body = self.parse_block_expression();

        Expression::While {
            condition: condition.into(),
            body: body.into(),
            span: self.span_from_tokens(start),
            needs_label: false,
        }
    }

    fn parse_while_let(&mut self, start: crate::lex::Token) -> Expression {
        self.ensure(Let);

        let pattern = self.parse_pattern_allowing_or();
        self.ensure(Equal);
        let scrutinee = self.with_control_flow_header(|p| p.parse_expression());
        let body = self.parse_block_expression();

        Expression::WhileLet {
            pattern,
            scrutinee: scrutinee.into(),
            body: body.into(),
            typed_pattern: None,
            span: self.span_from_tokens(start),
            needs_label: false,
        }
    }

    pub(super) fn parse_loop(&mut self) -> Expression {
        let start = self.current_token();

        self.ensure(Loop);
        let body = self.parse_block_expression();

        Expression::Loop {
            body: body.into(),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
            needs_label: false,
        }
    }

    pub(super) fn parse_break(&mut self) -> Expression {
        let start = self.current_token();

        self.next();

        let value = match self.current_token().kind {
            Semicolon | RightCurlyBrace | Comma | EOF => None,
            _ => Some(Box::new(self.parse_expression())),
        };

        Expression::Break {
            value,
            span: self.span_from_tokens(start),
        }
    }

    pub(super) fn parse_continue(&mut self) -> Expression {
        let start = self.current_token();

        self.next();

        Expression::Continue {
            span: self.span_from_tokens(start),
        }
    }
}
