use super::{MAX_TUPLE_ARITY, ParamMode, Parser};
use crate::EcoString;
use crate::ast::{Annotation, Attribute, Expression, Generic, Span, Visibility};
use crate::lex::Token;
use crate::lex::TokenKind::*;
use crate::types::Type;

impl<'source> Parser<'source> {
    pub fn parse_annotation(&mut self) -> Annotation {
        if !self.enter_recursion() {
            self.resync_on_error();
            return Annotation::Unknown;
        }
        let result = self.parse_annotation_inner();
        self.leave_recursion();
        result
    }

    fn parse_annotation_inner(&mut self) -> Annotation {
        match self.current_token().kind {
            Function => self.parse_function_annotation(),
            LeftParen => self.parse_tuple_annotation(),
            LeftSquareBracket => {
                let start = self.current_token();
                self.next();
                if self.advance_if(RightSquareBracket) {
                    let type_token = self.current_token();
                    let type_name = if type_token.kind == Identifier {
                        type_token.text.to_string()
                    } else {
                        "T".to_string()
                    };
                    let span_end = if type_token.kind == Identifier {
                        type_token.byte_offset + type_token.byte_length
                    } else {
                        start.byte_offset + 2
                    };
                    let error_span = Span::new(
                        self.file_id,
                        start.byte_offset,
                        span_end - start.byte_offset,
                    );
                    self.track_error_at(
                        error_span,
                        "invalid syntax for `Slice`",
                        format!("Use `Slice<{}>` instead of `[]{}`", type_name, type_name),
                    );
                    if self.current_token().kind == Identifier {
                        return self.parse_named_annotation();
                    }
                    return Annotation::Constructor {
                        name: "Slice".into(),
                        params: vec![],
                        span: error_span,
                    };
                }
                let span = self.span_from_tokens(start);
                self.track_error("unexpected `[` in type", "Use `Slice<T>` for slice types.");
                Annotation::Constructor {
                    name: "".into(),
                    params: vec![],
                    span,
                }
            }
            _ => self.parse_named_annotation(),
        }
    }

    fn parse_named_annotation(&mut self) -> Annotation {
        let start = self.current_token();
        let name = self.read_identifier_sequence();

        if self.advance_if(LeftAngleBracket) {
            let mut type_params = vec![];

            while self.can_start_annotation() {
                type_params.push(self.parse_annotation());
                match self.current_token().kind {
                    RightAngleBracket | ShiftRight => break,
                    Comma => self.next(),
                    _ => break,
                }
                if self.is_right_angle_like() {
                    self.track_error("expected type", "Add a type or remove the trailing comma.");
                }
            }

            if !self.advance_if_right_angle() {
                self.track_error("expected `>`", "Add `>` to close the type arguments.");
            }

            return Annotation::Constructor {
                name,
                params: type_params,
                span: self.span_from_tokens(start),
            };
        }

        if matches!(self.current_token().kind, LeftParen | LeftSquareBracket) {
            return self.parse_misdelimited_type_args(name, start);
        }

        Annotation::Constructor {
            name,
            params: vec![],
            span: self.span_from_tokens(start),
        }
    }

    fn parse_misdelimited_type_args(
        &mut self,
        name: EcoString,
        start: Token<'source>,
    ) -> Annotation {
        let open = self.current_token();
        let close_kind = if open.kind == LeftParen {
            RightParen
        } else {
            RightSquareBracket
        };
        let open_end = (open.byte_offset + open.byte_length) as usize;
        self.next();

        let mut type_params = vec![];
        while self.current_token().kind != close_kind && !self.at_eof() {
            if !self.can_start_annotation() {
                break;
            }
            type_params.push(self.parse_annotation());
            if !self.advance_if(Comma) {
                break;
            }
        }

        while self.current_token().kind != close_kind && !self.at_eof() {
            self.next();
        }

        let close = self.current_token();
        let close_start = close.byte_offset as usize;
        let consumed = self.advance_if(close_kind);
        let close_end = if consumed {
            (close.byte_offset + close.byte_length) as usize
        } else {
            close_start
        };

        let inner = self.source[open_end..close_start].trim();
        let corrected = format!("{}<{}>", name, inner);
        let original = &self.source[start.byte_offset as usize..close_end];

        let label_span = Span::new(
            self.file_id,
            open.byte_offset,
            (close_end as u32).saturating_sub(open.byte_offset),
        );
        self.error_angle_brackets_for_generics(
            label_span,
            format!("Write `{corrected}` instead of `{original}`"),
        );

        Annotation::Constructor {
            name,
            params: type_params,
            span: self.span_from_tokens(start),
        }
    }

    fn parse_function_annotation(&mut self) -> Annotation {
        let start = self.current_token();
        self.ensure(Function);
        self.ensure(LeftParen);

        let mut params = vec![];

        while self.is_not(RightParen) {
            params.push(self.parse_annotation());
            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        let return_type = self.parse_function_return_annotation();

        Annotation::Function {
            params,
            return_type: return_type.into(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_tuple_annotation(&mut self) -> Annotation {
        let start = self.current_token();
        self.ensure(LeftParen);

        let mut annotations = vec![];
        let mut has_trailing_comma = false;

        while self.is_not(RightParen) {
            annotations.push(self.parse_annotation());
            has_trailing_comma = self.is(Comma);
            self.expect_comma_or(RightParen);
        }

        self.ensure(RightParen);

        let span = self.span_from_tokens(start);

        if annotations.is_empty() {
            return Annotation::unit();
        }

        if annotations.len() == 1 {
            if has_trailing_comma {
                self.error_tuple_arity(1, span);
            }
            return annotations.into_iter().next().expect("len is 1");
        }

        if annotations.len() > MAX_TUPLE_ARITY {
            self.error_tuple_arity(annotations.len(), span);
        }

        Annotation::Tuple {
            elements: annotations,
            span,
        }
    }

    pub fn parse_generics(&mut self) -> Vec<Generic> {
        if !self.advance_if(LeftAngleBracket) {
            return vec![];
        }

        let mut generics = vec![];

        while !self.is_right_angle_like() && !self.at_eof() {
            generics.push(self.parse_generic());
            self.expect_comma_or(RightAngleBracket);
        }

        if !self.advance_if_right_angle() {
            self.ensure(RightAngleBracket);
        }

        generics
    }

    fn parse_generic(&mut self) -> Generic {
        let start = self.current_token();

        Generic {
            name: self.read_identifier(),
            bounds: self.parse_generic_bounds(),
            span: self.span_from_tokens(start),
        }
    }

    fn parse_generic_bounds(&mut self) -> Vec<Annotation> {
        if !self.advance_if(Colon) {
            return vec![];
        }

        if self.is_right_angle_like() || self.is(Comma) {
            self.track_error(
                "expected bound after `:`",
                "Provide a bound like `T: Display`.",
            );
            return vec![];
        }

        let mut bounds = vec![];

        while !self.is_right_angle_like() && self.is_not(Comma) {
            bounds.push(self.parse_annotation());
            if self.is_right_angle_like() || self.is(Comma) {
                break;
            }
            if !self.advance_if(Plus) {
                self.track_error(
                    "missing `+` between bounds",
                    "Use `+` to separate multiple bounds.",
                );
                break;
            }
            if self.is_right_angle_like() || self.is(Comma) {
                self.track_error(
                    "expected bound after `+`",
                    "Provide a bound or remove the trailing `+`.",
                );
            }
        }

        bounds
    }

    pub fn parse_function_return_annotation(&mut self) -> Annotation {
        if self.advance_if(Arrow) {
            return self.parse_annotation();
        }

        Annotation::Unknown
    }

    pub fn parse_interface_method(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<crate::ast::Attribute>,
    ) -> Expression {
        self.ensure(Function);

        let start = self.current_token();
        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();

        if self.is(LeftAngleBracket) {
            let generics_start = self.current_token();
            let generics = self.parse_generics(); // consume and discard
            let generics_span = self.span_from_tokens(generics_start);
            self.error_interface_method_with_type_parameters(generics_span, generics.len());
        }

        Expression::Function {
            doc,
            attributes,
            name,
            name_span,
            generics: vec![],
            params: self.parse_function_params(ParamMode::Strict),
            return_annotation: self.parse_function_return_annotation(),
            return_type: Type::uninferred(),
            visibility: Visibility::Private,
            body: Expression::NoOp.into(),
            ty: Type::uninferred(),
            span: self.span_from_tokens(start),
        }
    }

    pub fn parse_type_alias_with_doc(
        &mut self,
        doc: Option<std::string::String>,
        attributes: Vec<Attribute>,
    ) -> Expression {
        let start = self.current_token();

        self.ensure(Type);

        let name_token = self.current_token();
        let name_span = Span::new(self.file_id, name_token.byte_offset, name_token.byte_length);
        let name = self.read_identifier();
        let generics = self.parse_generics();

        let annotation = if self.advance_if(Equal) {
            self.parse_annotation()
        } else {
            Annotation::Opaque {
                span: self.span_from_tokens(start),
            }
        };

        Expression::TypeAlias {
            doc,
            attributes,
            name,
            name_span,
            generics,
            annotation,
            ty: Type::uninferred(),
            visibility: Visibility::Private,
            span: self.span_from_tokens(start),
        }
    }
}
