pub use token::{Token, TokenKind};
pub use types::{LexResult, Trivia};

use crate::parse::ParseError;

mod errors;
mod token;
mod types;

pub struct Lexer<'source> {
    input: &'source str,
    input_bytes: &'source [u8],
    current_offset: usize,
    file_id: u32,
    errors: Vec<ParseError>,
    pending_tokens: Vec<Token<'source>>,
    trivia: Trivia,
    last_newline_offset: Option<usize>,
}

impl<'source> Lexer<'source> {
    pub fn new(input: &'source str, file_id: u32) -> Lexer<'source> {
        Lexer {
            input,
            input_bytes: input.as_bytes(),
            current_offset: 0,
            file_id,
            errors: vec![],
            pending_tokens: vec![],
            trivia: Trivia::default(),
            last_newline_offset: None,
        }
    }

    pub fn lex(mut self) -> LexResult<'source> {
        let mut tokens = Vec::new();

        loop {
            if let Some(token) = self.pending_tokens.pop() {
                tokens.push(token);
                continue;
            }

            self.skip_whitespace();

            if self.at_eof() {
                tokens.push(self.eof_token());
                break;
            }

            if self.current_byte() == b'f' && self.peek_byte() == b'"' {
                let mut fstring_tokens = self.lex_format_string_tokens();
                fstring_tokens.reverse();
                self.pending_tokens = fstring_tokens;
                continue;
            }

            let token = self.create_token();
            tokens.push(token);
        }

        let tokens = self.insert_semicolons(tokens);

        LexResult {
            tokens,
            errors: self.errors,
            trivia: self.trivia,
        }
    }

    fn insert_semicolons(&self, tokens: Vec<Token<'source>>) -> Vec<Token<'source>> {
        let mut result = Vec::with_capacity(tokens.len() + tokens.len() / 4);

        for i in 0..tokens.len() {
            let token = tokens[i];
            result.push(token);

            if !Self::triggers_asi(token.kind) {
                continue;
            }

            if let Some(next_token) = self.find_next_non_comment_token(&tokens, i + 1) {
                if Self::continues_expression(next_token.kind) {
                    continue;
                }

                let token_end = (token.byte_offset + token.byte_length) as usize;
                if self.has_newline_between(token_end, next_token.byte_offset as usize) {
                    result.push(self.make_synthetic_semicolon(token_end));
                }
            }
        }

        result
    }

    fn triggers_asi(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Identifier
                | TokenKind::Integer
                | TokenKind::Imaginary
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::Char
                | TokenKind::Boolean
                | TokenKind::RightParen
                | TokenKind::RightSquareBracket
                | TokenKind::RightCurlyBrace
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Return
                | TokenKind::DotDot
                | TokenKind::DotDotEqual
                | TokenKind::QuestionMark
        )
    }

    fn continues_expression(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Plus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Pipeline
                | TokenKind::AmpersandDouble
                | TokenKind::PipeDouble
                | TokenKind::EqualDouble
                | TokenKind::NotEqual
                | TokenKind::LeftAngleBracket
                | TokenKind::RightAngleBracket
                | TokenKind::LessThanOrEqual
                | TokenKind::GreaterThanOrEqual
                | TokenKind::Dot
                | TokenKind::Equal
                | TokenKind::PlusEqual
                | TokenKind::MinusEqual
                | TokenKind::StarEqual
                | TokenKind::SlashEqual
                | TokenKind::Else
                | TokenKind::LeftCurlyBrace
                | TokenKind::RightCurlyBrace
                | TokenKind::RightParen
                | TokenKind::RightSquareBracket
                | TokenKind::As
        )
    }

    fn find_next_non_comment_token<'a>(
        &self,
        tokens: &'a [Token<'source>],
        start_index: usize,
    ) -> Option<&'a Token<'source>> {
        tokens
            .iter()
            .skip(start_index)
            .find(|&token| token.kind != TokenKind::Comment && token.kind != TokenKind::DocComment)
    }

    fn has_newline_between(&self, start: usize, end: usize) -> bool {
        self.input[start..end].contains('\n')
    }

    fn make_synthetic_semicolon(&self, position: usize) -> Token<'source> {
        Token {
            kind: TokenKind::Semicolon,
            text: "",
            byte_offset: position as u32,
            byte_length: 0,
        }
    }

    fn create_token(&mut self) -> Token<'source> {
        if let Some(token) = self.lex_lookahead_symbol() {
            return token;
        }

        let c = self.current_char();
        match c {
            '0'..='9' => self.lex_number(),
            _ if c.is_alphabetic() || c == '_' => self.lex_identifier(),
            '"' => self.lex_string_literal(),
            '`' => self.lex_backtick_literal(),
            '\'' => self.lex_char(),
            '/' => self.lex_slash(),
            ';' => self.semicolon_token(),
            '@' => self.lex_directive(),
            _ => self.handle_unexpected_char(),
        }
    }

    #[inline]
    fn current_byte(&self) -> u8 {
        if self.current_offset < self.input_bytes.len() {
            self.input_bytes[self.current_offset]
        } else {
            0
        }
    }

    #[inline]
    fn current_char(&self) -> char {
        self.input[self.current_offset..]
            .chars()
            .next()
            .unwrap_or('\0')
    }

    #[inline]
    fn peek_byte(&self) -> u8 {
        if self.current_offset + 1 < self.input_bytes.len() {
            self.input_bytes[self.current_offset + 1]
        } else {
            0
        }
    }

    #[inline]
    fn peek_char(&self) -> char {
        let next_offset = if self.current_byte() < 128 {
            self.current_offset + 1
        } else {
            self.current_offset + self.current_char().len_utf8()
        };
        self.input[next_offset..].chars().next().unwrap_or('\0')
    }

    fn peek_char_n(&self, n: usize) -> char {
        let mut offset = self.current_offset;
        for _ in 0..n {
            if offset >= self.input.len() {
                return '\0';
            }
            let c = self.input[offset..].chars().next().unwrap_or('\0');
            offset += c.len_utf8();
        }
        self.input[offset..].chars().next().unwrap_or('\0')
    }

    fn next(&mut self) {
        if self.at_eof() {
            return;
        }
        if self.current_byte() < 128 {
            self.current_offset += 1;
        } else {
            self.current_offset += self.current_char().len_utf8();
        }
    }

    fn skip(&mut self, count: usize) {
        for _ in 0..count {
            self.next();
        }
    }

    fn skip_whitespace(&mut self) {
        while !self.at_eof() && self.current_byte().is_ascii_whitespace() {
            if self.current_byte() == b'\n' {
                self.record_newline();
            }
            self.next();
        }
    }

    fn skip_horizontal_whitespace(&mut self) {
        while !self.at_eof() && matches!(self.current_byte(), b' ' | b'\t') {
            self.next();
        }
    }

    fn record_newline(&mut self) {
        let offset = self.current_offset;

        if let Some(last) = self.last_newline_offset {
            let between = &self.input[last + 1..offset];
            let is_blank = between.is_empty()
                || between
                    .chars()
                    .all(|c| c.is_ascii_whitespace() && c != '\n');
            if is_blank {
                self.trivia.blank_lines.push(offset as u32);
            }
        }

        self.last_newline_offset = Some(offset);
    }

    fn at_eof(&self) -> bool {
        self.current_offset >= self.input.len()
    }

    fn previous_char(&self) -> char {
        if self.current_offset == 0 {
            return '\0';
        }
        self.input[..self.current_offset]
            .chars()
            .next_back()
            .unwrap_or('\0')
    }

    fn resync_on_error(&mut self) {
        while !self.at_eof() {
            let byte = self.current_byte();

            if byte == b';' || byte == b'}' {
                break;
            }

            self.next();
        }
    }

    /// Lex a symbol that requires a lookahead to disambiguate, e.g. `=` and `==`
    fn lex_lookahead_symbol(&mut self) -> Option<Token<'source>> {
        let start_offset = self.current_offset;
        let current_char = self.current_char();
        let next_char = self.peek_char();
        let third_char = self.peek_char_n(2);

        if let Some(kind) = TokenKind::from_three_char_symbol(current_char, next_char, third_char) {
            self.skip(3);
            let end_offset = self.current_offset;
            return Some(Token {
                kind,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            });
        }

        if let Some(kind) = TokenKind::from_two_char_symbol(current_char, next_char) {
            self.skip(2);
            let end_offset = self.current_offset;
            return Some(Token {
                kind,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            });
        }

        if let Some(kind) = TokenKind::from_one_char_symbol(current_char) {
            self.next();
            let end_offset = self.current_offset;
            return Some(Token {
                kind,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            });
        }

        None
    }

    fn lex_number(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        if self.current_byte() == b'0' {
            let next = self.peek_byte();
            match next {
                b'x' | b'X' => {
                    self.next(); // consume '0'
                    self.next(); // consume 'x'
                    return self.lex_hex_number(start_offset);
                }
                b'o' | b'O' => {
                    self.next(); // consume '0'
                    self.next(); // consume 'o'
                    return self.lex_octal_number(start_offset);
                }
                b'b' | b'B' => {
                    self.next(); // consume '0'
                    self.next(); // consume 'b'
                    return self.lex_binary_number(start_offset);
                }
                b'0'..=b'7' => {
                    return self.lex_legacy_octal_number(start_offset);
                }
                _ => {} // decimal zero or float
            }
        }

        let mut kind = TokenKind::Integer;

        while !self.at_eof() {
            let byte = self.current_byte();
            if byte.is_ascii_digit() || byte == b'_' {
                if byte == b'_' && self.previous_char() == '_' {
                    let underscore_start = self.current_offset - 1;
                    self.error_consecutive_underscores(underscore_start);
                }
                self.next();
            } else {
                break;
            }
        }

        if self.previous_char() == '_' {
            self.error_number_trailing_underscore(
                self.current_offset - self.previous_char().len_utf8(),
            );
        }

        // Skip decimal part if preceded by single `.` (e.g., `tuple.0.0` — don't lex `0.0` as float).
        // Don't skip if preceded by `..` (range operator), e.g. `0..1.5` should lex `1.5` as float.
        let preceded_by_dot = start_offset > 0
            && self.input_bytes[start_offset - 1] == b'.'
            && !(start_offset > 1 && self.input_bytes[start_offset - 2] == b'.');

        if !preceded_by_dot
            && self.current_byte() == b'.'
            && self.peek_byte() != b'.'
            && (self.peek_byte().is_ascii_digit() || self.peek_byte() == b'_')
        {
            kind = TokenKind::Float;
            self.next();

            if self.current_byte() == b'_' {
                self.error_decimal_leading_underscore(self.current_offset);
            }

            while !self.at_eof() {
                let byte = self.current_byte();
                if byte.is_ascii_digit() || byte == b'_' {
                    if byte == b'_' && self.previous_char() == '_' {
                        let underscore_start = self.current_offset - 1;
                        self.error_consecutive_underscores(underscore_start);
                    }
                    self.next();
                } else {
                    break;
                }
            }

            if self.previous_char() == '_' {
                self.error_number_trailing_underscore(
                    self.current_offset - self.previous_char().len_utf8(),
                );
            }
        }

        if self.current_byte() == b'e' || self.current_byte() == b'E' {
            kind = TokenKind::Float;
            let exponent_start = self.current_offset;
            self.next(); // consume 'e' or 'E'

            if self.current_byte() == b'+' || self.current_byte() == b'-' {
                self.next();
            }

            if !self.current_byte().is_ascii_digit() {
                self.error_missing_exponent_digits(
                    exponent_start,
                    self.current_offset - exponent_start,
                );
            }

            while !self.at_eof() {
                let byte = self.current_byte();
                if byte.is_ascii_digit() || byte == b'_' {
                    if byte == b'_' && self.previous_char() == '_' {
                        let underscore_start = self.current_offset - 1;
                        self.error_consecutive_underscores(underscore_start);
                    }
                    self.next();
                } else {
                    break;
                }
            }

            if self.previous_char() == '_' {
                self.error_number_trailing_underscore(
                    self.current_offset - self.previous_char().len_utf8(),
                );
            }
        }

        if self.current_byte() == b'i' && !self.peek_byte().is_ascii_alphanumeric() {
            self.next(); // consume 'i'
            let end_offset = self.current_offset;
            return Token {
                kind: TokenKind::Imaginary,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        let end_offset = self.current_offset;
        Token {
            kind,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_hex_number(&mut self, start_offset: usize) -> Token<'source> {
        let digits_start = self.current_offset;

        while !self.at_eof() {
            let byte = self.current_byte();
            if byte.is_ascii_hexdigit() || byte == b'_' {
                if byte == b'_' && self.previous_char() == '_' {
                    let underscore_start = self.current_offset - 1;
                    self.error_consecutive_underscores(underscore_start);
                }
                self.next();
            } else {
                break;
            }
        }

        if self.current_offset == digits_start {
            self.error_missing_hex_digits(start_offset, 2);
        }

        if self.previous_char() == '_' {
            self.error_number_trailing_underscore(
                self.current_offset - self.previous_char().len_utf8(),
            );
        }

        if self.current_byte() == b'i' && !self.peek_byte().is_ascii_alphanumeric() {
            self.next(); // consume 'i'
            let end_offset = self.current_offset;
            self.error_non_decimal_imaginary("hex", start_offset, end_offset - start_offset);
            return Token {
                kind: TokenKind::Imaginary,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Integer,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_octal_number(&mut self, start_offset: usize) -> Token<'source> {
        let digits_start = self.current_offset;

        while !self.at_eof() {
            let byte = self.current_byte();
            if (b'0'..=b'7').contains(&byte) || byte == b'_' {
                if byte == b'_' && self.previous_char() == '_' {
                    let underscore_start = self.current_offset - 1;
                    self.error_consecutive_underscores(underscore_start);
                }
                self.next();
            } else if byte == b'8' || byte == b'9' {
                self.error_invalid_octal_digit(self.current_offset);
                self.next();
            } else {
                break;
            }
        }

        if self.current_offset == digits_start {
            self.error_missing_octal_digits(start_offset, 2);
        }

        if self.previous_char() == '_' {
            self.error_number_trailing_underscore(
                self.current_offset - self.previous_char().len_utf8(),
            );
        }

        if self.current_byte() == b'i' && !self.peek_byte().is_ascii_alphanumeric() {
            self.next(); // consume 'i'
            let end_offset = self.current_offset;
            self.error_non_decimal_imaginary("octal", start_offset, end_offset - start_offset);
            return Token {
                kind: TokenKind::Imaginary,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Integer,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_legacy_octal_number(&mut self, start_offset: usize) -> Token<'source> {
        self.next();

        while !self.at_eof() {
            let byte = self.current_byte();
            if (b'0'..=b'7').contains(&byte) || byte == b'_' {
                if byte == b'_' && self.previous_char() == '_' {
                    let underscore_start = self.current_offset - 1;
                    self.error_consecutive_underscores(underscore_start);
                }
                self.next();
            } else if byte == b'8' || byte == b'9' {
                self.error_invalid_octal_digit(self.current_offset);
                self.next();
            } else {
                break;
            }
        }

        if self.previous_char() == '_' {
            self.error_number_trailing_underscore(
                self.current_offset - self.previous_char().len_utf8(),
            );
        }

        if self.current_byte() == b'i' && !self.peek_byte().is_ascii_alphanumeric() {
            self.next();
            let end_offset = self.current_offset;
            self.error_non_decimal_imaginary("octal", start_offset, end_offset - start_offset);
            return Token {
                kind: TokenKind::Imaginary,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Integer,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_binary_number(&mut self, start_offset: usize) -> Token<'source> {
        let digits_start = self.current_offset;

        while !self.at_eof() {
            let byte = self.current_byte();
            if byte == b'0' || byte == b'1' || byte == b'_' {
                if byte == b'_' && self.previous_char() == '_' {
                    let underscore_start = self.current_offset - 1;
                    self.error_consecutive_underscores(underscore_start);
                }
                self.next();
            } else if (b'2'..=b'9').contains(&byte) {
                self.error_invalid_binary_digit(self.current_offset);
                self.next();
            } else {
                break;
            }
        }

        if self.current_offset == digits_start {
            self.error_missing_binary_digits(start_offset, 2);
        }

        if self.previous_char() == '_' {
            self.error_number_trailing_underscore(
                self.current_offset - self.previous_char().len_utf8(),
            );
        }

        if self.current_byte() == b'i' && !self.peek_byte().is_ascii_alphanumeric() {
            self.next();
            let end_offset = self.current_offset;
            self.error_non_decimal_imaginary("binary", start_offset, end_offset - start_offset);
            return Token {
                kind: TokenKind::Imaginary,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Integer,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_identifier(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        while !self.at_eof() {
            let c = self.current_char();
            if c.is_alphanumeric() || c == '_' {
                self.next();
            } else {
                break;
            }
        }

        let end_offset = self.current_offset;
        let text = &self.input[start_offset..end_offset];

        let kind = match text {
            "true" | "false" => TokenKind::Boolean,
            _ => TokenKind::from_keyword(text).unwrap_or(TokenKind::Identifier),
        };

        Token {
            kind,
            text,
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_backtick_literal(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.next();

        let mut terminated = false;

        while !self.at_eof() {
            let byte = self.current_byte();
            if byte == b'`' {
                terminated = true;
                self.next();
                break;
            } else if byte == b'\n' {
                break;
            }
            self.next();
        }

        let end_offset = self.current_offset;
        let length = end_offset - start_offset;

        if !terminated {
            self.error_unterminated_backtick(start_offset, length);
        }

        Token {
            kind: TokenKind::Backtick,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: length as u32,
        }
    }

    /// Consume up to 2 more octal digits after the first has already been read.
    fn consume_octal_escape(&mut self, first_digit: u8) -> u16 {
        let mut value: u16 = (first_digit - b'0') as u16;
        for _ in 0..2 {
            if self.at_eof() {
                break;
            }
            match self.current_byte() {
                d @ b'0'..=b'7' => {
                    value = value * 8 + (d - b'0') as u16;
                    self.next();
                }
                _ => break,
            }
        }
        value
    }

    fn lex_string_literal(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.next();

        let mut escaped = false;
        let mut terminated = false;

        while !self.at_eof() && !terminated {
            let byte = self.current_byte();
            if escaped {
                match byte {
                    b'0'..=b'7' => {
                        let escape_start = self.current_offset - 1;
                        self.next();
                        let value = self.consume_octal_escape(byte);
                        if value > 255 {
                            let escape_len = self.current_offset - escape_start;
                            self.error_octal_escape_out_of_range(escape_start, escape_len);
                        }
                        escaped = false;
                        continue;
                    }
                    b'n' | b't' | b'r' | b'\\' | b'"' | b'x' | b'u' | b'U' => {}
                    b'\'' => {}
                    _ => {
                        self.error_invalid_escape(start_offset, self.current_char());
                    }
                }
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                terminated = true;
                self.next();
                break;
            } else if byte == b'\n' {
                break; // unterminated string literal across newline, handled below
            }

            self.next();
        }

        let end_offset = self.current_offset;
        let length = end_offset - start_offset;

        if escaped {
            self.error_unterminated_escape(start_offset);
        }

        if !terminated {
            self.error_unterminated_string(start_offset, length);
        }

        Token {
            kind: TokenKind::String,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: length as u32,
        }
    }

    fn push_format_string_text_if_needed(
        &self,
        tokens: &mut Vec<Token<'source>>,
        text_segment_start: usize,
    ) {
        if text_segment_start < self.current_offset {
            tokens.push(Token {
                kind: TokenKind::FormatStringText,
                text: &self.input[text_segment_start..self.current_offset],
                byte_offset: text_segment_start as u32,
                byte_length: (self.current_offset - text_segment_start) as u32,
            });
        }
    }

    fn lex_format_string_interpolation(
        &mut self,
        tokens: &mut Vec<Token<'source>>,
    ) -> Result<(), ()> {
        let interp_start = self.current_offset;
        self.next();

        tokens.push(Token {
            kind: TokenKind::FormatStringInterpolationStart,
            text: &self.input[interp_start..self.current_offset],
            byte_offset: interp_start as u32,
            byte_length: (self.current_offset - interp_start) as u32,
        });

        let Some(interpolation_end) = self.find_interpolation_boundary() else {
            if self.has_newline_between(interp_start, self.input.len()) {
                self.error_multiline_format_string_interpolation(interp_start);
            } else {
                self.error_unclosed_brace_in_format_string(interp_start);
            }
            self.skip_to_format_string_end();
            return Err(());
        };

        if self.has_newline_between(interp_start, interpolation_end) {
            self.error_multiline_format_string_interpolation(interp_start);
        }

        while self.current_offset < interpolation_end {
            self.skip_horizontal_whitespace();
            if self.current_offset >= interpolation_end {
                break;
            }

            if self.current_byte() == b'f' && self.peek_byte() == b'"' {
                let mut fstring_tokens = self.lex_format_string_tokens();
                tokens.append(&mut fstring_tokens);
            } else if self.current_byte() == b'\\' && self.peek_byte() == b'"' {
                self.error_escaped_quote_in_interpolation(self.current_offset);
                self.skip(2);
            } else {
                let token = self.create_token();
                tokens.push(token);
            }
        }

        let close_offset = self.current_offset;
        self.next();
        tokens.push(Token {
            kind: TokenKind::FormatStringInterpolationEnd,
            text: &self.input[close_offset..self.current_offset],
            byte_offset: close_offset as u32,
            byte_length: (self.current_offset - close_offset) as u32,
        });

        Ok(())
    }

    fn scan_interpolation(&self, start: usize) -> Option<usize> {
        let bytes = self.input.as_bytes();
        let mut p = start;
        let mut depth = 1;

        while p < bytes.len() && depth > 0 {
            match bytes[p] {
                b'{' => {
                    depth += 1;
                    p += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth > 0 {
                        p += 1;
                    }
                }
                b'"' | b'\'' | b'`' => p = self.scan_past_quoted(p, bytes[p])?,
                b'f' if matches!(bytes.get(p + 1), Some(b'"')) => {
                    p = self.scan_past_fstring(p)?;
                }
                b'\\' => p += 2,
                b'/' if matches!(bytes.get(p + 1), Some(b'/')) => return None,
                b'\n' => return None,
                _ => p += 1,
            }
        }

        (depth == 0).then_some(p)
    }

    fn find_interpolation_boundary(&self) -> Option<usize> {
        self.scan_interpolation(self.current_offset)
    }

    fn scan_past_quoted(&self, start: usize, delimiter: u8) -> Option<usize> {
        let bytes = self.input.as_bytes();
        let mut p = start + 1;
        while p < bytes.len() {
            match bytes[p] {
                b'\\' if delimiter != b'`' => p += 2,
                b'\n' => return None,
                b if b == delimiter => return Some(p + 1),
                _ => p += 1,
            }
        }
        None
    }

    fn scan_past_fstring(&self, position: usize) -> Option<usize> {
        let bytes = self.input.as_bytes();
        let mut p = position + 2; // skip f"
        while p < bytes.len() {
            match bytes[p] {
                b'\\' => p += 2,
                b'{' if matches!(bytes.get(p + 1), Some(b'{')) => p += 2,
                b'}' if matches!(bytes.get(p + 1), Some(b'}')) => p += 2,
                b'{' => {
                    p = self.scan_interpolation(p + 1)?;
                    p += 1;
                }
                b'"' => return Some(p + 1),
                b'\n' => return None,
                _ => p += 1,
            }
        }
        None
    }

    fn skip_to_format_string_end(&mut self) {
        while !self.at_eof() {
            match self.current_byte() {
                b'"' => {
                    self.next();
                    return;
                }
                b'\n' => return,
                _ => self.next(),
            }
        }
    }

    fn lex_format_string_tokens(&mut self) -> Vec<Token<'source>> {
        let start_offset = self.current_offset;
        let mut tokens = Vec::new();

        self.skip(2);

        let fstring_start_end = self.current_offset;
        tokens.push(Token {
            kind: TokenKind::FormatStringStart,
            text: &self.input[start_offset..fstring_start_end],
            byte_offset: start_offset as u32,
            byte_length: (fstring_start_end - start_offset) as u32,
        });

        let mut text_segment_start = self.current_offset;

        while !self.at_eof() {
            let byte = self.current_byte();

            match byte {
                b'\\' if !self.at_eof() => {
                    let escape_start = self.current_offset;
                    self.next();
                    if !self.at_eof() {
                        let b = self.current_byte();
                        self.next();
                        if matches!(b, b'0'..=b'7') {
                            let value = self.consume_octal_escape(b);
                            if value > 255 {
                                let escape_len = self.current_offset - escape_start;
                                self.error_octal_escape_out_of_range(escape_start, escape_len);
                            }
                        }
                    }
                }
                b'{' if self.peek_byte() == b'{' => {
                    self.skip(2);
                }
                b'}' if self.peek_byte() == b'}' => {
                    self.skip(2);
                }
                b'"' => {
                    self.push_format_string_text_if_needed(&mut tokens, text_segment_start);

                    let end_offset = self.current_offset;
                    self.next();

                    tokens.push(Token {
                        kind: TokenKind::FormatStringEnd,
                        text: &self.input[end_offset..self.current_offset],
                        byte_offset: end_offset as u32,
                        byte_length: (self.current_offset - end_offset) as u32,
                    });
                    return tokens;
                }

                b'\n' => {
                    let length = self.current_offset.saturating_sub(start_offset);
                    self.error_unterminated_format_string(start_offset, length);
                    return tokens;
                }

                b'{' => {
                    self.push_format_string_text_if_needed(&mut tokens, text_segment_start);

                    if self.lex_format_string_interpolation(&mut tokens).is_err() {
                        return tokens;
                    }
                    text_segment_start = self.current_offset;
                }
                b'}' => {
                    self.error_unmatched_brace_in_format_string(self.current_offset);
                    self.next();
                }
                _ => {
                    self.next();
                }
            }
        }

        let length = self.current_offset.saturating_sub(start_offset);
        self.error_unterminated_format_string(start_offset, length);
        tokens
    }

    fn lex_char(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.next();

        if self.at_eof() || self.current_byte() == b'\'' {
            self.error_empty_rune_literal(start_offset);
            let end_offset = self.current_offset;
            return Token {
                kind: TokenKind::Char,
                text: &self.input[start_offset..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        if self.current_byte() != b'\\' {
            self.next();
        } else {
            self.next();

            if self.at_eof() {
                self.error_unterminated_escape(start_offset);
                let end_offset = self.current_offset;
                return Token {
                    kind: TokenKind::Char,
                    text: &self.input[start_offset..end_offset],
                    byte_offset: start_offset as u32,
                    byte_length: (end_offset - start_offset) as u32,
                };
            }

            match self.current_byte() {
                b'0'..=b'7' => {
                    let escape_start = self.current_offset - 1;
                    let first = self.current_byte();
                    self.next();
                    let value = self.consume_octal_escape(first);
                    if value > 255 {
                        let escape_len = self.current_offset - escape_start;
                        self.error_octal_escape_out_of_range(escape_start, escape_len);
                    }
                }
                b'n' | b't' | b'r' | b'\\' | b'\'' | b'x' => {
                    self.next();
                }
                _ => {
                    self.error_invalid_escape(start_offset, self.current_char());

                    while !self.at_eof() && self.current_byte() != b'\'' {
                        self.next();
                    }

                    if !self.at_eof() && self.current_byte() == b'\'' {
                        self.next();
                    }

                    let end_offset = self.current_offset;
                    return Token {
                        kind: TokenKind::Char,
                        text: &self.input[start_offset..end_offset],
                        byte_offset: start_offset as u32,
                        byte_length: (end_offset - start_offset) as u32,
                    };
                }
            }
        }

        if self.at_eof() || self.current_byte() != b'\'' {
            let length = self.current_offset - start_offset;
            self.error_unterminated_rune(start_offset, length);
        }

        if !self.at_eof() && self.current_byte() == b'\'' {
            self.next();
        }

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Char,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn lex_slash(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        if self.peek_byte() != b'/' {
            self.next();
            return Token {
                kind: TokenKind::Slash,
                text: &self.input[start_offset..self.current_offset],
                byte_offset: start_offset as u32,
                byte_length: 1,
            };
        }

        let slash_count = self.count_consecutive(b'/');

        if slash_count >= 4 {
            self.error_excess_slashes_in_comment(start_offset, slash_count);
        }

        self.skip(slash_count);

        if slash_count == 3 {
            if self.current_byte() == b' ' {
                self.next();
            }
            let text_start = self.current_offset;
            self.skip_to_eol();
            let end_offset = self.current_offset;

            self.trivia
                .doc_comments
                .push((start_offset as u32, end_offset as u32));

            return Token {
                kind: TokenKind::DocComment,
                text: &self.input[text_start..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        self.skip_to_eol();
        let end_offset = self.current_offset;

        self.trivia
            .comments
            .push((start_offset as u32, end_offset as u32));

        Token {
            kind: TokenKind::Comment,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn count_consecutive(&self, byte: u8) -> usize {
        let mut count = 0;
        let mut offset = self.current_offset;
        while offset < self.input_bytes.len() && self.input_bytes[offset] == byte {
            count += 1;
            offset += 1;
        }
        count
    }

    fn skip_to_eol(&mut self) {
        while !self.at_eof() && self.current_byte() != b'\n' {
            self.next();
        }
    }

    fn lex_directive(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.next();

        while !self.at_eof() {
            let byte = self.current_byte();
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.next();
            } else {
                break;
            }
        }

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Directive,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn handle_unexpected_char(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.error_unexpected_char(self.current_offset, self.current_char());

        self.resync_on_error();

        let end_offset = self.current_offset;

        Token {
            kind: TokenKind::Error,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn eof_token(&self) -> Token<'source> {
        Token {
            kind: TokenKind::EOF,
            text: &self.input[self.current_offset..self.current_offset],
            byte_offset: self.current_offset as u32,
            byte_length: 0,
        }
    }

    fn semicolon_token(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.next();

        Token {
            kind: TokenKind::Semicolon,
            text: &self.input[start_offset..self.current_offset],
            byte_offset: start_offset as u32,
            byte_length: (self.current_offset - start_offset) as u32,
        }
    }
}
