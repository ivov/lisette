use ecow::EcoString;

use super::{ParseError, Parser};
use crate::ast;
use crate::lex::TokenKind::{self, *};
use crate::types::Type;

const RANGE_PREC: u8 = 6;
const CAST_PREC: u8 = 9;

impl<'source> Parser<'source> {
    /// Parses by grouping together operations in expressions based on precedence.
    ///
    /// 1. Parse a left-hand side expression (primary, unary, or prefix).
    /// 2. Look for binary or postfix operators.
    /// 3. For binary operators: If the operator's precedence is higher than `min_prec`,
    ///    parse the right-hand side recursively with the operator's precedence.
    /// 4. For postfix operators: Transform the current expression into a larger one.
    ///
    /// The `min_prec` param sets the minimum precedence level for this parsing context.
    pub fn pratt_parse(&mut self, min_prec: u8) -> ast::Expression {
        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            self.resync_on_error();
            return ast::Expression::Unit {
                ty: Type::uninferred(),
                span,
            };
        }

        let start = self.current_token();
        let mut lhs = self.parse_left_hand_side();
        let depth_before_loop = self.depth;

        while !self.at_eof() && !self.too_many_errors() {
            if self.check_go_channel_send() {
                self.depth = depth_before_loop;
                self.leave_recursion();
                return lhs;
            }

            if self.at_range() && RANGE_PREC > min_prec {
                lhs = self.parse_range(Some(lhs.into()), start);
                continue;
            }

            if self.current_token().kind == As && CAST_PREC > min_prec {
                self.next();
                let target_type = self.parse_annotation();
                lhs = ast::Expression::Cast {
                    expression: lhs.into(),
                    target_type,
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                };
                continue;
            }

            if min_prec == 0
                && self.current_token().kind == PipeDouble
                && self.newline_before_current()
            {
                break;
            }

            if let Some(prec) = self.binary_operator_precedence(self.current_token().kind)
                && prec > min_prec
            {
                let operator = self.parse_binary_operator();
                let rhs = self.pratt_parse(prec);
                lhs = ast::Expression::Binary {
                    operator,
                    left: lhs.into(),
                    right: rhs.into(),
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                };
                continue;
            }

            if self.is_postfix_operator(&lhs) {
                if !self.enter_recursion() {
                    break;
                }
                if self.is_format_string(&lhs)
                    && (self.current_token().kind == LeftParen
                        || self.current_token().kind == LeftSquareBracket)
                    && self.newline_before_current()
                {
                    break;
                }
                lhs = self.include_in_larger_expression(lhs);
                continue;
            }

            if matches!(self.current_token().kind, Ampersand | Pipe | Caret)
                && !self.newline_before_current()
            {
                let op_token = self.current_token();
                let span = self.span_from_token(op_token);
                let error = ParseError::new(
                    "Unsupported operator",
                    span,
                    format!("`{}` is not a supported binary operator", op_token.text),
                )
                .with_help("Lisette does not support bitwise operators")
                .with_parse_code("unsupported_operator");
                self.errors.push(error);
                self.next();
                let _rhs = self.pratt_parse(min_prec);
                continue;
            }

            break;
        }

        self.depth = depth_before_loop;
        self.leave_recursion();

        lhs
    }

    fn prefix_operator_precedence(&self, kind: TokenKind) -> u8 {
        match kind {
            Minus | Bang | Ampersand => 15,
            _ => {
                debug_assert!(false, "unexpected prefix operator: {:?}", kind);
                15
            }
        }
    }

    fn binary_operator_precedence(&self, kind: TokenKind) -> Option<u8> {
        match kind {
            LeftAngleBracket if self.is_type_args_call() => None,
            Pipeline => Some(1),
            PipeDouble if self.stream.peek_ahead(1).kind == Arrow => None,
            PipeDouble => Some(3),
            AmpersandDouble => Some(4),
            EqualDouble | NotEqual | LeftAngleBracket | RightAngleBracket | LessThanOrEqual
            | GreaterThanOrEqual => Some(5),
            Plus | Minus => Some(7),
            Star | Slash | Percent => Some(8),
            _ => None,
        }
    }

    fn is_postfix_operator(&self, lhs: &ast::Expression) -> bool {
        match self.current_token().kind {
            LeftParen | LeftSquareBracket | QuestionMark | Dot => true,
            LeftCurlyBrace => match lhs {
                ast::Expression::Identifier { .. } | ast::Expression::DotAccess { .. } => {
                    self.is_struct_instantiation()
                }
                _ => false,
            },
            LeftAngleBracket => self.is_type_args_call(),
            Colon if self.stream.peek_ahead(1).kind == Colon => true,
            _ => false,
        }
    }

    fn is_format_string(&self, expression: &ast::Expression) -> bool {
        matches!(
            expression,
            ast::Expression::Literal {
                literal: ast::Literal::FormatString(_),
                ..
            }
        )
    }

    fn parse_left_hand_side(&mut self) -> ast::Expression {
        let start = self.current_token();

        match start.kind {
            Bang | Minus => {
                self.next();

                let operator = if start.kind == Bang {
                    ast::UnaryOperator::Not
                } else {
                    ast::UnaryOperator::Negative
                };

                let prec = self.prefix_operator_precedence(start.kind);

                ast::Expression::Unary {
                    operator,
                    expression: self.pratt_parse(prec).into(),
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                }
            }

            Ampersand => {
                self.next();
                if self.current_token().kind == Mut {
                    let span = ast::Span::new(
                        self.file_id,
                        start.byte_offset,
                        self.current_token().byte_offset + self.current_token().byte_length
                            - start.byte_offset,
                    );
                    self.track_error_at(
                        span,
                        "invalid syntax",
                        "Lisette has no mutable references. Use `&x` instead",
                    );
                    self.next(); // consume `mut`
                }
                let prec = self.prefix_operator_precedence(start.kind);
                ast::Expression::Reference {
                    expression: self.pratt_parse(prec).into(),
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                }
            }

            _ => self.parse_atomic_expression(),
        }
    }

    pub fn include_in_larger_expression(&mut self, lhs: ast::Expression) -> ast::Expression {
        match self.current_token().kind {
            LeftParen => self.parse_function_call(lhs, vec![]),
            LeftSquareBracket => self.parse_index_expression(lhs),
            LeftCurlyBrace => self.parse_struct_call(lhs),
            QuestionMark => self.parse_try(lhs),
            Dot => self.parse_field_access(lhs),
            LeftAngleBracket => {
                let type_args = self.parse_type_args();

                if self.current_token().kind == Dot && self.stream.peek_ahead(1).kind == Identifier
                {
                    let type_name = match &lhs {
                        ast::Expression::Identifier { value, .. } => value.as_str(),
                        ast::Expression::DotAccess { member, .. } => member.as_str(),
                        _ => "",
                    };
                    let method = self.stream.peek_ahead(1).text;
                    let args_str = type_args
                        .iter()
                        .map(format_annotation)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let plural = type_args.len() != 1;
                    let title = if plural {
                        "Misplaced type arguments"
                    } else {
                        "Misplaced type argument"
                    };
                    let help = if !type_name.is_empty() {
                        format!(
                            "Set the type {} on the method: `{}.{}<{}>()`",
                            if plural { "arguments" } else { "argument" },
                            type_name,
                            method,
                            args_str,
                        )
                    } else {
                        format!(
                            "Set the type {} on the method: `.{}<{}>()`",
                            if plural { "arguments" } else { "argument" },
                            method,
                            args_str,
                        )
                    };
                    let Some(first) = type_args.first() else {
                        return self.parse_function_call(lhs, type_args);
                    };
                    let first_span = first.get_span();
                    let last_span = type_args.last().expect("non-empty").get_span();
                    let span = ast::Span::new(
                        self.file_id,
                        first_span.byte_offset,
                        (last_span.byte_offset + last_span.byte_length)
                            .saturating_sub(first_span.byte_offset),
                    );
                    let error = ParseError::new(title, span, "misplaced")
                        .with_parse_code("syntax_error")
                        .with_help(help);
                    self.errors.push(error);

                    let dot_access = self.parse_field_access(lhs);
                    return self.parse_function_call(dot_access, type_args);
                }

                self.parse_function_call(lhs, type_args)
            }

            Colon => {
                let lhs_name = match &lhs {
                    ast::Expression::Identifier { value, .. } => value.to_string(),
                    ast::Expression::DotAccess { member, .. } => member.to_string(),
                    _ => std::string::String::new(),
                };
                let colon_token = self.current_token();
                let span = ast::Span::new(self.file_id, colon_token.byte_offset, 2);
                let after = self.stream.peek_ahead(2);

                if after.kind == LeftAngleBracket {
                    let help = if !lhs_name.is_empty() {
                        format!(
                            "Lisette does not use turbofish syntax. Use `{}<T>(...)` instead",
                            lhs_name
                        )
                    } else {
                        "Lisette does not use turbofish syntax. Use `func<T>(...)` instead"
                            .to_string()
                    };
                    self.track_error_at(span, "invalid syntax", help);
                    self.next(); // consume first `:`
                    self.next(); // consume second `:`
                    let type_args = self.parse_type_args();
                    self.parse_function_call(lhs, type_args)
                } else {
                    let help = if !lhs_name.is_empty() && after.kind == Identifier {
                        format!(
                            "Use `.` instead of `::` for enum variant access, e.g. `{}.{}`",
                            lhs_name, after.text
                        )
                    } else {
                        "Use `.` instead of `::` for enum variant access".to_string()
                    };
                    self.track_error_at(span, "invalid syntax", help);
                    self.next(); // consume first `:`
                    self.next(); // consume second `:`
                    let field_start = self.current_token();
                    let field: EcoString = self.current_token().text.into();
                    self.ensure(Identifier);
                    ast::Expression::DotAccess {
                        ty: Type::uninferred(),
                        expression: lhs.into(),
                        member: field,
                        span: self.span_from_tokens(field_start),
                        dot_access_kind: None,
                        receiver_coercion: None,
                    }
                }
            }

            _ => {
                debug_assert!(
                    false,
                    "is_postfix_operator and include_in_larger_expression are out of sync"
                );
                self.track_error("internal error", "Unexpected token in postfix position");
                self.resync_on_error();
                lhs
            }
        }
    }

    pub fn parse_range_end(&mut self) -> ast::Expression {
        self.pratt_parse(RANGE_PREC)
    }

    fn check_go_channel_send(&mut self) -> bool {
        if self.current_token().kind != LeftAngleBracket {
            return false;
        }
        let next = self.stream.peek_ahead(1);
        if next.kind != Minus {
            return false;
        }
        let current = self.current_token();
        if current.byte_offset + current.byte_length != next.byte_offset {
            return false;
        }

        let span = ast::Span::new(
            self.file_id,
            self.current_token().byte_offset,
            self.current_token().byte_length + 1,
        );
        self.track_error_at(
            span,
            "invalid syntax",
            "Use `ch.Send(value)` inside a `select` expression",
        );
        self.resync_on_error();
        true
    }
}

fn format_annotation(ann: &ast::Annotation) -> std::string::String {
    match ann {
        ast::Annotation::Constructor { name, params, .. } => {
            if params.is_empty() {
                name.to_string()
            } else {
                format!(
                    "{}<{}>",
                    name,
                    params
                        .iter()
                        .map(format_annotation)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        ast::Annotation::Tuple { elements, .. } => {
            format!(
                "({})",
                elements
                    .iter()
                    .map(format_annotation)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        ast::Annotation::Function {
            params,
            return_type,
            ..
        } => {
            format!(
                "fn({}) -> {}",
                params
                    .iter()
                    .map(format_annotation)
                    .collect::<Vec<_>>()
                    .join(", "),
                format_annotation(return_type)
            )
        }
        ast::Annotation::Unknown | ast::Annotation::Opaque { .. } => "_".to_string(),
    }
}
