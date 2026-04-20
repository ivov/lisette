use ecow::EcoString;

use super::{MAX_TUPLE_ARITY, ParseError, Parser};
use crate::ast::{Annotation, Binding, Literal, Pattern, RestPattern, Span, StructFieldPattern};
use crate::lex::Token;
use crate::lex::TokenKind::*;
use crate::types::Type;

impl<'source> Parser<'source> {
    pub fn parse_pattern_allowing_or(&mut self) -> Pattern {
        let start = self.current_token();
        let first = self.parse_pattern();

        if self.is_not(Pipe) {
            return first;
        }

        let mut patterns = vec![first];
        while self.advance_if(Pipe) {
            patterns.push(self.parse_pattern());
        }

        Pattern::Or {
            patterns,
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_pattern(&mut self) -> Pattern {
        if !self.enter_recursion() {
            let span = self.span_from_token(self.current_token());
            self.resync_on_error();
            return Pattern::WildCard { span };
        }
        let start = self.current_token();
        let mut result = self.parse_pattern_inner();
        if self.advance_if(As) {
            if !self.is(Identifier) {
                self.track_error("expected identifier after `as`", "Use `as <name>`");
            } else if self.current_token().text == "_" {
                self.track_error(
                    "`_` is not a valid `as` alias",
                    "Use a named binding, or omit `as _`",
                );
                self.next();
            } else {
                let name: EcoString = self.current_token().text.into();
                self.next();
                result = Pattern::AsBinding {
                    pattern: Box::new(result),
                    name,
                    span: self.span_from_tokens(start),
                };
            }
        }
        self.leave_recursion();
        result
    }

    fn parse_pattern_inner(&mut self) -> Pattern {
        let start = self.current_token();

        if self.current_token().kind.is_keyword() {
            let keyword = self.current_token().text.to_string();
            let span = self.span_from_token(start);
            let error = ParseError::new("Reserved keyword", span, "reserved keyword")
                .with_parse_code("keyword_as_binding")
                .with_help(format!("Rename binding `{}`", keyword));
            self.errors.push(error);
            self.next();
            return Pattern::Identifier {
                identifier: keyword.into(),
                span,
            };
        }

        match self.current_token().kind {
            Integer => self.parse_integer_pattern(),
            Float => self.parse_float_pattern(),
            Boolean => self.parse_boolean_pattern(),
            String => self.parse_string_pattern(),
            Char => self.parse_char_pattern(),

            Imaginary => {
                self.track_error(
                    "not allowed",
                    "Imaginary literals are not supported in patterns",
                );
                self.next();
                Pattern::WildCard {
                    span: self.span_from_tokens(start),
                }
            }

            LeftParen => self.parse_tuple_or_unit_pattern(),

            LeftSquareBracket => self.parse_slice_pattern(),

            Identifier => self.parse_identifier_based_pattern(),

            Minus => self.parse_negative_pattern(),

            _ => {
                self.unexpected_token("pattern");
                Pattern::WildCard {
                    span: self.span_from_tokens(start),
                }
            }
        }
    }

    fn parse_negative_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        self.next();

        match self.current_token().kind {
            Integer => {
                let int_pattern = self.parse_integer_pattern();
                let Pattern::Literal {
                    literal: Literal::Integer { value, text },
                    ..
                } = int_pattern
                else {
                    return int_pattern;
                };
                let span = self.span_from_tokens(start);
                if value > i64::MIN.unsigned_abs() {
                    self.track_error_at(
                        span,
                        "negative integer out of range",
                        "Negative integer must be ≥ -9223372036854775808 (i64 minimum).",
                    );
                    return Pattern::WildCard { span };
                }
                let neg_text = match text {
                    Some(t) => format!("-{t}"),
                    None => format!("-{value}"),
                };
                Pattern::Literal {
                    literal: Literal::Integer {
                        value: value.wrapping_neg(),
                        text: Some(neg_text),
                    },
                    ty: Type::uninferred(),
                    span,
                }
            }
            Float => {
                let span = self.span_from_tokens(start);
                self.track_error_at(
                    span,
                    "not allowed",
                    "Float literals are not supported in patterns",
                );
                self.next();
                Pattern::WildCard {
                    span: self.span_from_tokens(start),
                }
            }
            _ => {
                self.track_error(
                    "expected number after `-`",
                    "Negative patterns require a number, e.g., `-5`",
                );
                Pattern::WildCard {
                    span: self.span_from_tokens(start),
                }
            }
        }
    }

    fn parse_nested_pattern(&mut self) -> Pattern {
        let pattern = self.parse_pattern();

        if self.is(Pipe) {
            let token = self.current_token();
            let span = Span::new(self.file_id, token.byte_offset, token.byte_length);
            self.emit_nested_or_error(span);

            while self.is(Pipe) {
                self.next(); // consume `|`
                self.parse_pattern();
            }
        }

        pattern
    }

    fn emit_nested_or_error(&mut self, span: Span) {
        let error = ParseError::new("Invalid or-pattern", span, "or-pattern not allowed here")
            .with_parse_code("nested_or_pattern")
            .with_help("Use `Ok(x) | Ok(y)` instead of `Ok(x | y)`");
        self.errors.push(error);
    }

    fn check_nested_or_pattern(&mut self, pattern: &Pattern) {
        if let Pattern::Or { span, .. } = pattern {
            self.emit_nested_or_error(*span);
        }
    }

    fn parse_integer_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        let text = start.text;
        let literal = self.parse_integer_text(text);
        self.next();

        Pattern::Literal {
            literal,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_float_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        let float_text = start.text.to_string();
        self.next();

        let span = self.span_from_tokens(start);
        self.error_float_pattern_not_allowed(span, &float_text);

        Pattern::WildCard { span }
    }

    fn parse_boolean_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        let b = start.text == "true";
        self.next();

        Pattern::Literal {
            literal: Literal::Boolean(b),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_string_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        let s = start.text;
        self.next();
        let s_stripped = if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            s[1..s.len() - 1].to_string()
        } else {
            s.to_string()
        };

        Pattern::Literal {
            literal: Literal::String(s_stripped),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_char_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        let s = start.text;
        self.next();
        let char_str = if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
            s[1..s.len() - 1].to_string()
        } else {
            s.to_string()
        };

        Pattern::Literal {
            literal: Literal::Char(char_str),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_tuple_or_unit_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        self.ensure(LeftParen);

        if self.advance_if(RightParen) {
            return Pattern::Unit {
                ty: Type::uninferred(),
                span: self.span_from_tokens(start),
            };
        }

        let first = self.parse_pattern_allowing_or();

        if self.advance_if(RightParen) {
            if matches!(first, Pattern::Or { .. }) {
                self.check_nested_or_pattern(&first);
            }
            return first;
        }

        if matches!(first, Pattern::Or { .. }) {
            self.check_nested_or_pattern(&first);
        }

        let mut elements = vec![first];
        self.expect_comma_or(RightParen);

        while self.is_not(RightParen) {
            elements.push(self.parse_nested_pattern());
            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        let span = self.span_from_tokens(start);

        if elements.len() > MAX_TUPLE_ARITY {
            self.error_tuple_arity(elements.len(), span);
        }

        Pattern::Tuple { elements, span }
    }

    fn parse_slice_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        self.ensure(LeftSquareBracket);

        let mut elements = Vec::new();
        let mut rest = RestPattern::Absent;

        while self.is_not(RightSquareBracket) {
            if let Some((binding, rest_start)) = self.try_parse_rest() {
                if rest.is_present() {
                    self.track_error(
                        "multiple rest patterns in slice pattern",
                        "Only one `..` or `..rest` is allowed.",
                    );
                } else {
                    rest = match binding {
                        Some(name) => RestPattern::Bind {
                            name,
                            span: self.span_from_tokens(rest_start),
                        },
                        None => RestPattern::Discard(self.span_from_tokens(rest_start)),
                    };
                }
                self.expect_comma_or(RightSquareBracket);
                continue;
            }

            if rest.is_present() {
                let suffix_start = self.current_token();
                self.parse_pattern();
                let suffix_span = self.span_from_tokens(suffix_start);
                let error = ParseError::new("Invalid pattern", suffix_span, "not supported")
                    .with_parse_code("suffix_slice_pattern")
                    .with_help("Use `[first, ..rest]` instead of `[..rest, last]`.")
                    .with_note("Elements after rest pattern are not supported.");
                self.errors.push(error);
                self.expect_comma_or(RightSquareBracket);
                continue;
            }

            elements.push(self.parse_nested_pattern());
            self.expect_comma_or(RightSquareBracket);
        }

        let span = self.span_from_tokens(start);
        self.ensure(RightSquareBracket);

        Pattern::Slice {
            prefix: elements,
            rest,
            element_ty: Type::uninferred(),
            span,
        }
    }

    fn parse_identifier_based_pattern(&mut self) -> Pattern {
        let start = self.current_token();
        let name = self.current_token().text.to_string();
        self.next();

        let full_name = if self.is(Dot) {
            self.parse_qualified_pattern_name(name)
        } else if self.is(Colon) && self.stream.peek_ahead(1).kind == Colon {
            let colon_token = self.current_token();
            let span = Span::new(self.file_id, colon_token.byte_offset, 2);
            let after = self.stream.peek_ahead(2);
            let example = if after.kind == Identifier {
                format!("{}.{}", name, after.text)
            } else {
                format!("{}.<variant>", name)
            };
            self.track_error_at(
                span,
                "invalid syntax",
                format!(
                    "Use `.` instead of `::` for enum variant access, e.g. `{}`",
                    example
                ),
            );
            self.next(); // consume first `:`
            self.next(); // consume second `:`
            let mut full_name = name;
            if self.is(Identifier) {
                full_name.push('.');
                full_name.push_str(self.current_token().text);
                self.next();
            }
            self.parse_qualified_pattern_name(full_name)
        } else {
            name.clone()
        };

        match self.current_token().kind {
            LeftCurlyBrace => self.parse_struct_pattern(full_name, start),
            LeftParen => self.parse_enum_variant_pattern(full_name, start),
            _ => {
                let span = self.span_from_tokens(start);
                if full_name == "_" {
                    Pattern::WildCard { span }
                } else if full_name.contains('.') || self.is_uppercase(&full_name) {
                    Pattern::EnumVariant {
                        identifier: full_name.into(),
                        fields: vec![],
                        rest: false,
                        ty: Type::uninferred(),
                        span,
                    }
                } else {
                    Pattern::Identifier {
                        identifier: full_name.into(),
                        span,
                    }
                }
            }
        }
    }

    fn parse_qualified_pattern_name(
        &mut self,
        initial: std::string::String,
    ) -> std::string::String {
        let mut name = initial;

        while self.advance_if(Dot) {
            if self.is_not(Identifier) {
                break;
            }
            name.push('.');
            name.push_str(self.current_token().text);
            self.next();
        }

        name
    }

    fn parse_struct_pattern(
        &mut self,
        name: std::string::String,
        start: Token<'source>,
    ) -> Pattern {
        self.ensure(LeftCurlyBrace);

        let mut fields = Vec::new();
        let mut seen_fields: Vec<(EcoString, Span)> = Vec::new();
        let mut rest = false;

        while self.is_not(RightCurlyBrace) {
            if self.advance_if(DotDot) {
                rest = true;
                if self.is(Identifier) {
                    self.next();
                }
                if self.advance_if(Comma) && self.is_not(RightCurlyBrace) {
                    self.track_error(
                        "cannot be last",
                        "Move the spread expression `..rest` to the last position in the struct",
                    );
                }
                break;
            }

            let field_start = self.current_token();
            let field_name = self.read_identifier();
            let field_name_span = self.span_from_tokens(field_start);

            if let Some((_, first_span)) = seen_fields.iter().find(|(n, _)| n == &field_name) {
                self.error_duplicate_field_in_pattern(&field_name, *first_span, field_name_span);
            }

            let field_pattern = if self.advance_if(Colon) {
                self.parse_nested_pattern()
            } else {
                let span = field_name_span;
                if field_name == "_" {
                    Pattern::WildCard { span }
                } else {
                    Pattern::Identifier {
                        identifier: field_name.clone(),
                        span,
                    }
                }
            };

            seen_fields.push((field_name.clone(), field_name_span));
            fields.push(StructFieldPattern {
                name: field_name,
                value: field_pattern,
            });

            self.expect_comma_or(RightCurlyBrace);
        }

        self.ensure(RightCurlyBrace);

        Pattern::Struct {
            identifier: name.into(),
            fields,
            rest,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_enum_variant_pattern(
        &mut self,
        name: std::string::String,
        start: Token<'source>,
    ) -> Pattern {
        self.ensure(LeftParen);

        let mut fields = Vec::new();
        let mut rest = false;

        while self.is_not(RightParen) {
            if self.advance_if(DotDot) {
                rest = true;
                self.advance_if(Comma);
                break;
            }
            fields.push(self.parse_nested_pattern());
            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        Pattern::EnumVariant {
            identifier: name.into(),
            fields,
            rest,
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_binding(&mut self) -> Binding {
        Binding {
            pattern: self.parse_pattern(),
            annotation: self.parse_optional_type_annotation(),
            typed_pattern: None,
            ty: Type::uninferred(),
            mutable: false,
        }
    }

    pub fn parse_binding_allowing_or(&mut self) -> Binding {
        Binding {
            pattern: self.parse_pattern_allowing_or(),
            annotation: self.parse_optional_type_annotation(),
            typed_pattern: None,
            ty: Type::uninferred(),
            mutable: false,
        }
    }

    fn parse_optional_type_annotation(&mut self) -> Option<Annotation> {
        if self.advance_if(Colon) {
            if self.can_start_annotation() {
                Some(self.parse_annotation())
            } else {
                self.track_error(
                    "expected type after `:`",
                    "Annotate the type, e.g. `x: int`.",
                );
                None
            }
        } else {
            None
        }
    }

    pub fn parse_binding_with_type(&mut self) -> Binding {
        if self.is_current_uppercase() && self.stream.peek_ahead(1).kind == Colon {
            let start = self.current_token();
            let name = start.text.to_string();
            self.next();

            let span = self.span_from_tokens(start);
            self.error_uppercase_binding(span);

            return Binding {
                pattern: Pattern::Identifier {
                    identifier: name.into(),
                    span,
                },
                annotation: self.parse_optional_type_annotation(),
                typed_pattern: None,
                ty: Type::uninferred(),
                mutable: false,
            };
        }

        if self.is(Ampersand) {
            let amp_token = self.current_token();
            let next = self.stream.peek_ahead(1);
            let is_mut_self = next.kind == Mut && self.stream.peek_ahead(2).text == "self";
            let is_ref_self = next.kind == Identifier && next.text == "self";

            if is_ref_self || is_mut_self {
                let span_len = if is_mut_self {
                    // &mut self
                    self.stream.peek_ahead(2).byte_offset + self.stream.peek_ahead(2).byte_length
                        - amp_token.byte_offset
                } else {
                    // &self
                    next.byte_offset + next.byte_length - amp_token.byte_offset
                };
                let span = Span::new(self.file_id, amp_token.byte_offset, span_len);
                self.track_error_at(
                    span,
                    "invalid syntax",
                    "Lisette methods receive `self` by reference. Use `self` instead",
                );
                self.next();
                if is_mut_self {
                    self.next();
                }
            }
        }

        let is_mut = self.advance_if(Mut);

        let pattern = self.parse_pattern();

        if let Pattern::Identifier { identifier, .. } = &pattern
            && identifier == "self"
            && self.is_not(Colon)
        {
            return Binding {
                pattern,
                annotation: None,
                typed_pattern: None,
                ty: Type::uninferred(),
                mutable: false,
            };
        }

        self.ensure(Colon);
        let annotation = self.parse_annotation();

        Binding {
            pattern,
            annotation: Some(annotation),
            typed_pattern: None,
            ty: Type::uninferred(),
            mutable: is_mut,
        }
    }

    fn try_parse_rest(&mut self) -> Option<(Option<EcoString>, Token<'source>)> {
        if self.is(DotDot) {
            let rest_start = self.current_token();
            self.ensure(DotDot);
            if self.is(Identifier) {
                let name: EcoString = self.current_token().text.into();
                self.next();
                return Some((Some(name), rest_start));
            }
            return Some((None, rest_start));
        }

        if self.is(Identifier) {
            let text = self.current_token().text;
            if let Some(binding) = text.strip_prefix("..") {
                let rest_start = self.current_token();
                self.next();
                let name = if binding.is_empty() {
                    None
                } else {
                    Some(EcoString::from(binding))
                };
                return Some((name, rest_start));
            }
        }

        None
    }

    fn is_uppercase(&self, identifier: &str) -> bool {
        identifier.chars().next().unwrap_or('a').is_uppercase()
    }

    fn is_current_uppercase(&self) -> bool {
        self.is(Identifier) && self.is_uppercase(self.current_token().text)
    }

    pub fn can_start_pattern(&self) -> bool {
        matches!(
            self.current_token().kind,
            Integer
                | Float
                | Boolean
                | String
                | Char
                | LeftParen
                | LeftSquareBracket
                | Identifier
                | Minus
        )
    }
}
