use crate::ast::{self, Span};
use crate::lex;
use crate::lex::TokenKind::*;
use crate::lex::{Token, TokenKind};
use crate::types::Type;

pub const MAX_TUPLE_ARITY: usize = 5;
pub const TUPLE_FIELDS: &[&str] = &["First", "Second", "Third", "Fourth", "Fifth"];
const MAX_DEPTH: u32 = 64;
const MAX_ERRORS: usize = 50;
const MAX_LOOKAHEAD: usize = 256;

mod annotations;
mod control_flow;
mod definitions;
mod directives;
mod error;
mod expressions;
mod identifiers;
mod patterns;
mod pratt;

pub use error::ParseError;

pub struct ParseResult {
    pub ast: Vec<ast::Expression>,
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    pub fn failed(&self) -> bool {
        !self.errors.is_empty()
    }
}

pub struct Parser<'source> {
    stream: TokenStream<'source>,
    previous_token: Token<'source>,
    pub errors: Vec<ParseError>,
    file_id: u32,
    in_control_flow_header: bool,
    source: &'source str,
    depth: u32,
}

impl<'source> Parser<'source> {
    pub fn new(tokens: Vec<Token<'source>>, source: &'source str) -> Parser<'source> {
        Self::with_file_id(tokens, source, 0)
    }

    pub fn lex_and_parse_file(source: &str, file_id: u32) -> ParseResult {
        let lex_result = lex::Lexer::new(source, file_id).lex();

        if lex_result.failed() {
            return ParseResult {
                ast: vec![],
                errors: lex_result.errors,
            };
        }

        Parser::with_file_id(lex_result.tokens, source, file_id).parse()
    }

    fn with_file_id(
        tokens: Vec<Token<'source>>,
        source: &'source str,
        file_id: u32,
    ) -> Parser<'source> {
        let stream = TokenStream::new(tokens);
        let first_token = stream.peek();

        let mut parser = Parser {
            stream,
            previous_token: first_token,
            errors: Default::default(),
            file_id,
            in_control_flow_header: false,
            source,
            depth: 0,
        };

        parser.skip_comments();

        parser
    }

    pub fn parse(mut self) -> ParseResult {
        let mut top_items = vec![];

        self.skip_comments();

        while !self.at_eof() && !self.too_many_errors() {
            let position = self.position();
            let item = self.parse_top_item();
            if !matches!(item, ast::Expression::Unit { .. }) {
                top_items.push(item);
            }
            self.advance_if(Semicolon);
            if self.position() == position {
                self.next();
            }
        }

        ParseResult {
            ast: top_items,
            errors: self.errors,
        }
    }

    pub fn parse_top_item(&mut self) -> ast::Expression {
        let doc_with_span = self.collect_doc_comments();

        let attributes = self.parse_attributes();

        let pub_token = if self.is(Pub) {
            Some(self.current_token())
        } else {
            None
        };
        let is_public = pub_token.is_some();
        if is_public {
            self.next();
        }

        if is_public && self.is(Impl) {
            let token = pub_token.unwrap();
            let span = ast::Span::new(self.file_id, token.byte_offset, token.byte_length);
            let error = ParseError::new("Misplaced `pub`", span, "not allowed here")
                .with_parse_code("syntax_error")
                .with_help("Place `pub` on individual methods inside the `impl` block instead");
            self.errors.push(error);
        }

        let is_documentable = matches!(
            self.current_token().kind,
            Enum | Struct | Interface | Function | Const | Var | Type
        );

        if let Some((_, ref span)) = doc_with_span
            && !is_documentable
        {
            self.error_detached_doc_comment(*span);
        }

        let doc = doc_with_span.map(|(text, _)| text);

        let expression = match self.current_token().kind {
            Enum => self.parse_enum_definition(doc, attributes),
            Struct => self.parse_struct_definition(doc, attributes),
            Interface => self.parse_interface_definition(doc),
            Function => self.parse_function(doc, attributes),
            Impl => self.parse_impl_block(),
            Const => self.parse_const_definition(doc),
            Var => self.parse_var_declaration(doc),
            Import => self.parse_import(),
            Type => self.parse_type_alias_with_doc(doc),
            Comment => {
                let start = self.current_token();
                self.skip_comments();
                ast::Expression::Unit {
                    ty: Type::uninferred(),
                    span: self.span_from_tokens(start),
                }
            }
            _ => self.unexpected_token("top_item"),
        };

        if is_public {
            return expression.set_public();
        }

        expression
    }

    pub fn parse_block_item(&mut self) -> ast::Expression {
        match self.current_token().kind {
            Enum => {
                self.track_error(
                    "misplaced",
                    "Move this enum definition to the top level of the file.",
                );
                self.parse_enum_definition(None, vec![])
            }
            Struct => {
                self.track_error(
                    "misplaced",
                    "Move this struct definition to the top level of the file.",
                );
                self.parse_struct_definition(None, vec![])
            }
            Type => {
                self.track_error(
                    "misplaced",
                    "Move this type alias to the top level of the file.",
                );
                self.parse_type_alias_with_doc(None)
            }
            Import => {
                self.track_error(
                    "misplaced",
                    "Move this import to the top level of the file.",
                );
                self.parse_import()
            }
            Impl => {
                self.track_error(
                    "misplaced",
                    "Move this `impl` block to the top level of the file.",
                );
                self.parse_impl_block()
            }
            Interface => {
                self.track_error(
                    "misplaced",
                    "Move this interface definition to the top level of the file.",
                );
                self.parse_interface_definition(None)
            }
            Function => self.parse_function(None, vec![]),
            Const => self.parse_const_definition(None),

            Let => self.parse_let(),
            Return => self.parse_return(),
            For => self.parse_for(),
            While => self.parse_while(),
            Loop => self.parse_loop(),
            Break => self.parse_break(),
            Continue => self.parse_continue(),
            Defer => self.parse_defer(),
            Directive => self.parse_directive(),
            _ => self.parse_assignment(),
        }
    }

    fn current_token(&self) -> Token<'source> {
        self.stream.peek()
    }

    fn newline_before_current(&self) -> bool {
        let prev_end = (self.previous_token.byte_offset + self.previous_token.byte_length) as usize;
        let curr_start = self.current_token().byte_offset as usize;
        if prev_end <= curr_start && curr_start <= self.source.len() {
            return self.source[prev_end..curr_start].contains('\n');
        }
        false
    }

    fn next(&mut self) {
        self.previous_token = self.current_token();
        self.stream.consume();
        self.skip_comments();
    }

    fn skip_comments(&mut self) {
        while self.is(Comment) {
            self.previous_token = self.current_token();
            self.stream.consume();
        }
    }

    fn collect_doc_comments(&mut self) -> Option<(std::string::String, ast::Span)> {
        let mut docs = Vec::new();
        let mut first_span: Option<ast::Span> = None;

        while self.is(DocComment) {
            let token = self.current_token();
            if first_span.is_none() {
                first_span = Some(self.span_from_token(token));
            }
            docs.push(token.text.to_string());
            self.previous_token = token;
            self.stream.consume();
            self.skip_comments();
        }

        if docs.is_empty() {
            None
        } else {
            Some((docs.join("\n"), first_span.unwrap()))
        }
    }

    fn expect_comma_or(&mut self, closing: TokenKind) {
        if self.is(Comma) || self.is(closing) || self.at_item_boundary() {
            self.advance_if(Comma);
            return;
        }

        self.track_error(
            format!("expected `,` or {}", closing),
            "Add a comma between elements.",
        );

        loop {
            if self.at_eof() || self.is(Comma) || self.is(closing) || self.at_item_boundary() {
                break;
            }
            self.next();
        }

        self.advance_if(Comma);
    }

    pub fn at_eof(&self) -> bool {
        self.is(EOF)
    }

    fn at_range(&self) -> bool {
        matches!(self.current_token().kind, DotDot | DotDotEqual)
    }

    fn advance_if(&mut self, token_kind: TokenKind) -> bool {
        if self.is(token_kind) {
            self.next();
            return true;
        }

        false
    }

    fn is(&self, token_kind: TokenKind) -> bool {
        self.current_token().kind == token_kind
    }

    fn is_not(&self, token_kind: TokenKind) -> bool {
        if self.at_eof() {
            return false;
        }

        self.current_token().kind != token_kind
    }

    fn ensure(&mut self, token_kind: TokenKind) {
        if self.current_token().kind != token_kind {
            self.track_ensure_error(token_kind);
        }

        if self.at_eof() {
            return;
        }

        self.next();
    }

    fn ensure_progress(&mut self, start_position: usize, closing: TokenKind) {
        if self.stream.position == start_position && self.is_not(closing) && !self.at_eof() {
            self.next();
        }
    }

    fn span_from_token(&self, token: Token<'source>) -> ast::Span {
        ast::Span::new(self.file_id, token.byte_offset, token.byte_length)
    }

    fn span_from_tokens(&self, start_token: Token<'source>) -> ast::Span {
        let end_byte_offset = self.previous_token.byte_offset + self.previous_token.byte_length;
        let byte_length = end_byte_offset.saturating_sub(start_token.byte_offset);

        ast::Span::new(self.file_id, start_token.byte_offset, byte_length)
    }

    fn span_from_offset(&self, start_byte_offset: u32) -> ast::Span {
        let end_byte_offset = self.previous_token.byte_offset + self.previous_token.byte_length;
        let byte_length = end_byte_offset.saturating_sub(start_byte_offset);

        ast::Span::new(self.file_id, start_byte_offset, byte_length)
    }

    fn is_type_args_call(&self) -> bool {
        let mut position = 1; // 0 is <
        let mut depth = 1;

        loop {
            if position > MAX_LOOKAHEAD {
                return false;
            }
            match self.stream.peek_ahead(position).kind {
                LeftAngleBracket => depth += 1,
                RightAngleBracket if depth == 1 => {
                    let next = self.stream.peek_ahead(position + 1).kind;
                    return next == LeftParen
                        || (next == Dot
                            && self.stream.peek_ahead(position + 2).kind == Identifier
                            && self.stream.peek_ahead(position + 3).kind == LeftParen);
                }
                RightAngleBracket => depth -= 1,
                LeftParen => {
                    let mut paren_depth = 1;
                    position += 1;
                    while paren_depth > 0 {
                        if position > MAX_LOOKAHEAD {
                            return false;
                        }
                        match self.stream.peek_ahead(position).kind {
                            LeftParen => paren_depth += 1,
                            RightParen => paren_depth -= 1,
                            EOF => return false,
                            _ => {}
                        }
                        position += 1;
                    }
                    continue;
                }
                EOF | Plus | Minus | Star | Slash | Percent | EqualDouble | NotEqual
                | AmpersandDouble | PipeDouble | Semicolon | LeftCurlyBrace | RightCurlyBrace
                | LeftSquareBracket | RightSquareBracket => return false,
                _ => {}
            }
            position += 1;
        }
    }

    fn has_block_after_struct(&self) -> bool {
        let mut depth = 1;
        let mut i = 0;
        while depth > 0 {
            i += 1;
            if i > MAX_LOOKAHEAD {
                return false;
            }
            let token = self.stream.peek_ahead(i);
            match token.kind {
                LeftCurlyBrace => depth += 1,
                RightCurlyBrace => depth -= 1,
                EOF => return false,
                _ => {}
            }
        }
        let after = self.stream.peek_ahead(i + 1);
        matches!(
            after.kind,
            LeftCurlyBrace
                | RightParen
                | EqualDouble
                | NotEqual
                | LeftAngleBracket
                | RightAngleBracket
                | LessThanOrEqual
                | GreaterThanOrEqual
                | AmpersandDouble
                | PipeDouble
                | Plus
                | Minus
                | Star
                | Slash
                | Percent
        )
    }

    fn is_struct_instantiation(&self) -> bool {
        if self.previous_token.kind != Identifier {
            return false;
        }

        let is_uppercase = self
            .previous_token
            .text
            .starts_with(|c: char| c.is_uppercase());
        let first_ahead = self.stream.peek_ahead(1);

        if first_ahead.kind == DotDot {
            return true;
        }

        if first_ahead.kind == RightCurlyBrace {
            if self.in_control_flow_header {
                return is_uppercase && self.has_block_after_struct();
            }
            return is_uppercase;
        }

        if first_ahead.kind == Identifier {
            let second_ahead = self.stream.peek_ahead(2);
            return match second_ahead.kind {
                Colon => self.stream.peek_ahead(3).kind != Colon,
                Comma | RightCurlyBrace => {
                    if self.in_control_flow_header {
                        is_uppercase && self.has_block_after_struct()
                    } else {
                        is_uppercase
                    }
                }
                _ => false,
            };
        }

        false
    }

    fn enter_recursion(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            let span = self.span_from_token(self.current_token());
            self.track_error_at(span, "too deeply nested", "Reduce nesting depth");
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave_recursion(&mut self) {
        self.depth -= 1;
    }

    fn too_many_errors(&self) -> bool {
        self.errors.len() >= MAX_ERRORS
    }

    fn position(&self) -> u32 {
        self.current_token().byte_offset
    }

    fn at_sync_point(&self) -> bool {
        matches!(
            self.current_token().kind,
            Semicolon
                | RightCurlyBrace
                | RightParen
                | RightSquareBracket
                | Comma
                | Function
                | Struct
                | Enum
                | Const
                | Impl
                | Interface
                | Type
                | Import
        )
    }

    fn can_start_annotation(&self) -> bool {
        matches!(self.current_token().kind, Identifier | Function | LeftParen)
    }

    fn at_item_boundary(&self) -> bool {
        matches!(
            self.current_token().kind,
            Let | Function | Struct | Enum | Impl | Interface | Type | Const | Import
        )
    }

    fn resync_on_error(&mut self) {
        if !self.at_eof() {
            self.next();
        }

        while !self.at_sync_point() && !self.at_eof() {
            self.next();
        }
    }

    fn track_error(
        &mut self,
        label: impl Into<std::string::String>,
        help: impl Into<std::string::String>,
    ) {
        let current = self.current_token();
        let span = ast::Span::new(self.file_id, current.byte_offset, current.byte_length);
        self.track_error_at(span, label, help);
    }

    fn track_error_at(
        &mut self,
        span: ast::Span,
        label: impl Into<std::string::String>,
        help: impl Into<std::string::String>,
    ) {
        if self.too_many_errors() {
            return;
        }
        let error = ParseError::new("Syntax error", span, label.into())
            .with_parse_code("syntax_error")
            .with_help(help.into());

        self.errors.push(error);
    }

    fn track_ensure_error(&mut self, expected_token: TokenKind) {
        if self.too_many_errors() {
            return;
        }
        let current = self.current_token();

        let error_code = match expected_token {
            Semicolon => "missing_semicolon",
            RightCurlyBrace => "unclosed_block",
            _ => "unexpected_token",
        };

        let span = ast::Span::new(self.file_id, current.byte_offset, current.byte_length);
        let error = ParseError::new("Syntax error", span, format!("expected {}", expected_token))
            .with_parse_code(error_code);

        self.errors.push(error);
    }

    fn close_brace_span(&mut self, start: Token<'source>, error_anchor: Token<'source>) -> Span {
        if self.is(RightCurlyBrace) {
            let close = self.current_token();
            self.next();
            let end = close.byte_offset + close.byte_length;
            Span::new(
                self.file_id,
                start.byte_offset,
                end.saturating_sub(start.byte_offset),
            )
        } else {
            self.error_unclosed_block(&error_anchor);
            self.span_from_tokens(start)
        }
    }

    fn error_unclosed_block(&mut self, open_brace: &Token) {
        let span = ast::Span::new(self.file_id, open_brace.byte_offset, open_brace.byte_length);
        let error = ParseError::new("Unclosed block", span, "opening brace here")
            .with_parse_code("unclosed_block")
            .with_help("Add a closing `}`");

        self.errors.push(error);
    }

    fn error_tuple_arity(&mut self, arity: usize, span: Span) {
        let help = if arity == 0 {
            "Use `()` for unit type".to_string()
        } else if arity == 1 {
            "Use the type directly without wrapping in a tuple".to_string()
        } else {
            "For >5 elements, use a struct with named fields".to_string()
        };

        let error = ParseError::new(
            "Invalid tuple",
            span,
            format!("{}-element tuple not allowed", arity),
        )
        .with_parse_code("tuple_element_count")
        .with_help(help);

        self.errors.push(error);
    }

    fn error_duplicate_field_in_pattern(
        &mut self,
        field_name: &str,
        first_span: Span,
        second_span: Span,
    ) {
        let error = ParseError::new(
            "Duplicate field",
            first_span,
            format!("first use of `{}`", field_name),
        )
        .with_span_label(second_span, "used again")
        .with_parse_code("duplicate_field_in_pattern")
        .with_help("Remove the duplicate binding");

        self.errors.push(error);
    }

    fn error_duplicate_impl_parent(&mut self, first_span: Span, second_span: Span) {
        let error = ParseError::new("Duplicate impl", first_span, "first use")
            .with_span_label(second_span, "used again")
            .with_parse_code("duplicate_impl_parent")
            .with_help("Remove the duplicate parent");

        self.errors.push(error);
    }

    fn error_duplicate_struct_field(&mut self, name: &str, first_span: Span, second_span: Span) {
        let error = ParseError::new("Duplicate field", first_span, "first defined")
            .with_span_label(second_span, "defined again")
            .with_parse_code("duplicate_struct_field")
            .with_help(format!("Remove the duplicate field `{}`", name));

        self.errors.push(error);
    }

    fn error_duplicate_enum_variant(&mut self, name: &str, first_span: Span, second_span: Span) {
        let error = ParseError::new("Duplicate variant", first_span, "first defined")
            .with_span_label(second_span, "defined again")
            .with_parse_code("duplicate_enum_variant")
            .with_help(format!("Remove the duplicate variant `{}`", name));

        self.errors.push(error);
    }

    fn error_duplicate_interface_method(
        &mut self,
        name: &str,
        first_span: Span,
        second_span: Span,
    ) {
        let error = ParseError::new("Duplicate method", first_span, "first defined")
            .with_span_label(second_span, "defined again")
            .with_parse_code("duplicate_interface_method")
            .with_help(format!("Remove the duplicate method `{}`", name));

        self.errors.push(error);
    }

    fn error_float_pattern_not_allowed(&mut self, span: Span, float_text: &str) {
        let error = ParseError::new("Invalid pattern", span, "float literal not allowed here")
            .with_parse_code("float_pattern")
            .with_help(format!(
                "Use a guard instead: `x if x == {} =>`",
                float_text
            ));

        self.errors.push(error);
    }

    fn error_uppercase_binding(&mut self, span: Span) {
        let error = ParseError::new("Invalid binding name", span, "uppercase not allowed here")
            .with_parse_code("uppercase_binding")
            .with_help("Lowercase the binding");

        self.errors.push(error);
    }

    fn error_detached_doc_comment(&mut self, span: Span) {
        let error = ParseError::new("Unattached doc comment", span, "is detached")
            .with_parse_code("detached_doc_comment")
            .with_help("Place the doc comment on the line immediately above a symbol definition");

        self.errors.push(error);
    }

    fn error_interface_method_with_type_parameters(&mut self, span: Span, count: usize) {
        let label = if count == 1 {
            "type parameter not allowed"
        } else {
            "type parameters not allowed"
        };
        let error = ParseError::new("Invalid interface method", span, label)
            .with_parse_code("interface_method_with_type_parameters")
            .with_help(
                "Interface methods cannot have type parameters, because Go interfaces do not support generic methods",
            );

        self.errors.push(error);
    }

    pub(crate) fn parse_integer_text(&mut self, text: &str) -> ast::Literal {
        self.parse_integer_text_with(text, false)
    }

    pub(crate) fn parse_integer_text_with(
        &mut self,
        text: &str,
        preserve_decimal_text: bool,
    ) -> ast::Literal {
        let clean = if text.contains('_') {
            std::borrow::Cow::Owned(text.replace('_', ""))
        } else {
            std::borrow::Cow::Borrowed(text)
        };

        let (n, is_decimal) = if clean.starts_with("0x") || clean.starts_with("0X") {
            let value = u64::from_str_radix(&clean[2..], 16).unwrap_or_else(|_| {
                self.track_error(
                    format!("hex literal '{text}' is too large"),
                    "Maximum value is `0xFFFFFFFFFFFFFFFF`.",
                );
                0
            });
            (value, false)
        } else if clean.starts_with("0o") || clean.starts_with("0O") {
            let value = u64::from_str_radix(&clean[2..], 8).unwrap_or_else(|_| {
                self.track_error(
                    format!("octal literal '{text}' is too large"),
                    "Maximum value is `0o1777777777777777777777`.",
                );
                0
            });
            (value, false)
        } else if clean.starts_with("0b") || clean.starts_with("0B") {
            let value = u64::from_str_radix(&clean[2..], 2).unwrap_or_else(|_| {
                self.track_error(
                    format!("binary literal '{text}' is too large"),
                    "Value must fit in 64 bits.",
                );
                0
            });
            (value, false)
        } else if clean.len() > 1
            && clean.starts_with('0')
            && clean.chars().skip(1).all(|c| c.is_ascii_digit())
        {
            let value = u64::from_str_radix(&clean[1..], 8).unwrap_or_else(|_| {
                self.track_error(
                    format!("octal literal '{text}' is too large"),
                    "Maximum value is `01777777777777777777777`.",
                );
                0
            });
            (value, false)
        } else {
            let value = clean.parse().unwrap_or_else(|_| {
                self.track_error(
                    format!("integer literal '{text}' is too large"),
                    "Maximum value is `18446744073709551615`.",
                );
                0
            });
            (value, true)
        };

        let original_text = if is_decimal && !preserve_decimal_text {
            None
        } else {
            Some(text.to_string())
        };

        ast::Literal::Integer {
            value: n,
            text: original_text,
        }
    }

    fn unexpected_token(&mut self, ctx: &str) -> ast::Expression {
        let token = self.current_token();
        let token_descriptor = if token.text.is_empty() {
            format!("{:?}", token.kind)
        } else {
            format!("`{}`", token.text)
        };

        let span = ast::Span::new(self.file_id, token.byte_offset, token.byte_length);

        let (label, error_code, help) = match ctx {
            "expr" => (
                format!("expected expression, found {}", token_descriptor),
                "expected_expression",
                "Check your syntax.",
            ),
            "pattern" => (
                format!("unexpected {} in pattern", token_descriptor),
                "invalid_pattern",
                "Patterns include literals, variables, and destructuring.",
            ),
            "literal" => (
                format!("expected literal, found {}", token_descriptor),
                "expected_literal",
                "Literals include numbers, strings, characters, and booleans.",
            ),
            "top_item" if token.text == "trait" => (
                format!("unexpected {}", token_descriptor),
                "trait_unsupported",
                "Lisette uses `interface` with Go-style structural typing. Types automatically satisfy interfaces if they have the required methods.",
            ),
            "top_item" if token.text == "use" => (
                "unexpected syntax for import".to_string(),
                "use_unsupported",
                "Use `import` instead of `use` for imports: `import \"module/path\"`",
            ),
            "top_item" => (
                "expected declaration".to_string(),
                "expected_declaration",
                "At the top level of a file, Lisette expects `fn`, `struct`, `enum`, `interface`, `import`, or `type`.",
            ),
            _ => (
                format!("unexpected {}", token_descriptor),
                "unexpected_token",
                "Check your syntax.",
            ),
        };

        let error = ParseError::new("Syntax error", span, label)
            .with_parse_code(error_code)
            .with_help(help);

        if !self.too_many_errors() {
            self.errors.push(error);
        }

        self.resync_on_error();

        ast::Expression::Unit {
            ty: Type::uninferred(),
            span,
        }
    }
}

struct TokenStream<'source> {
    tokens: Vec<Token<'source>>,
    position: usize,
}

impl<'source> TokenStream<'source> {
    fn new(tokens: Vec<Token<'source>>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn peek(&self) -> Token<'source> {
        self.tokens
            .get(self.position)
            .copied()
            .unwrap_or_else(|| Token {
                kind: TokenKind::EOF,
                text: "",
                byte_offset: self
                    .tokens
                    .last()
                    .map(|t| t.byte_offset + t.byte_length)
                    .unwrap_or(0),
                byte_length: 0,
            })
    }

    fn peek_ahead(&self, n: usize) -> Token<'source> {
        self.tokens
            .get(self.position + n)
            .copied()
            .unwrap_or_else(|| Token {
                kind: TokenKind::EOF,
                text: "",
                byte_offset: self
                    .tokens
                    .last()
                    .map(|t| t.byte_offset + t.byte_length)
                    .unwrap_or(0),
                byte_length: 0,
            })
    }

    fn consume(&mut self) -> Token<'source> {
        let token = self.peek();
        if self.position < self.tokens.len() {
            self.position += 1;
        }
        token
    }
}
