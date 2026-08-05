pub use token::{Token, TokenKind};
pub use types::LexResult;

use crate::parse::ParseError;
use std::borrow::Cow;
use std::iter::Peekable;
use std::str::Chars;

mod errors;
mod token;
mod types;

pub struct Lexer<'source> {
    input: &'source str,
    current_offset: usize,
    file_id: u32,
    errors: Vec<ParseError>,
    blank_lines: Vec<u32>,
    last_newline_offset: Option<usize>,
}

impl<'source> Lexer<'source> {
    pub fn new(input: &'source str, file_id: u32) -> Lexer<'source> {
        Lexer {
            input,
            current_offset: 0,
            file_id,
            errors: vec![],
            blank_lines: Vec::new(),
            last_newline_offset: None,
        }
    }

    pub fn lex(mut self) -> LexResult<'source> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();

            if self.at_eof() {
                tokens.push(self.eof_token());
                break;
            }

            if self.try_consume_unsupported_raw_variant(self.input.len()) {
                continue;
            }

            if self.current_byte() == b'f' && self.peek_byte() == b'"' {
                tokens.extend(self.lex_format_string_tokens());
                continue;
            }

            let token = self.create_token();
            tokens.push(token);
        }

        let tokens = self.insert_semicolons(tokens);

        LexResult {
            tokens,
            errors: self.errors,
            blank_lines: self.blank_lines,
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
                | TokenKind::RawString
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
                | TokenKind::AmpersandEqual
                | TokenKind::PipeEqual
                | TokenKind::CaretEqual
                | TokenKind::AndNotEqual
                | TokenKind::ShiftLeftEqual
                | TokenKind::ShiftRightEqual
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
        tokens.iter().skip(start_index).find(|&token| {
            !matches!(
                token.kind,
                TokenKind::Comment | TokenKind::DocComment | TokenKind::FileComment
            )
        })
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
            'r' if self.peek_byte() == b'"' => self.lex_raw_string_literal(),
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
        self.input
            .as_bytes()
            .get(self.current_offset)
            .copied()
            .unwrap_or(0)
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
        self.input
            .as_bytes()
            .get(self.current_offset + 1)
            .copied()
            .unwrap_or(0)
    }

    #[inline]
    fn peek_byte_at(&self, n: usize) -> u8 {
        let offset = self.current_offset + n;
        self.input.as_bytes().get(offset).copied().unwrap_or(0)
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
                self.blank_lines.push(offset as u32);
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
                _ => {} // decimal zero, leading-zero literal, or float
            }
        }

        let mut kind = TokenKind::Integer;

        self.scan_digits(|byte| byte.is_ascii_digit());
        self.check_trailing_underscore();

        if !self.preceded_by_single_dot(start_offset)
            && self.current_byte() == b'.'
            && self.peek_byte() != b'.'
            && (self.peek_byte().is_ascii_digit() || self.peek_byte() == b'_')
        {
            kind = TokenKind::Float;
            self.next();

            if self.current_byte() == b'_' {
                self.error_decimal_leading_underscore(self.current_offset);
            }

            self.scan_digits(|byte| byte.is_ascii_digit());
            self.check_trailing_underscore();
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

            self.scan_digits(|byte| byte.is_ascii_digit());
            self.check_trailing_underscore();
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

    /// Consume digit and underscore bytes, reporting consecutive underscores.
    fn scan_digits(&mut self, is_digit: impl Fn(u8) -> bool) {
        while !self.at_eof() {
            let byte = self.current_byte();
            if is_digit(byte) || byte == b'_' {
                if byte == b'_' && self.previous_char() == '_' {
                    let underscore_start = self.current_offset - 1;
                    self.error_consecutive_underscores(underscore_start);
                }
                self.next();
            } else {
                break;
            }
        }
    }

    fn check_trailing_underscore(&mut self) {
        if self.previous_char() == '_' {
            self.error_number_trailing_underscore(
                self.current_offset - self.previous_char().len_utf8(),
            );
        }
    }

    // A single preceding `.` is field access (e.g. `tuple.0.0`), so do not lex `0.0` as float.
    // A preceding `..` is the range operator, so e.g. `0..1.5` should lex `1.5` as float.
    fn preceded_by_single_dot(&self, start_offset: usize) -> bool {
        start_offset > 0
            && self.input.as_bytes()[start_offset - 1] == b'.'
            && !(start_offset > 1 && self.input.as_bytes()[start_offset - 2] == b'.')
    }

    fn finish_radix_number(
        &mut self,
        start_offset: usize,
        digits_start: usize,
        base: &str,
        error_missing_digits: fn(&mut Self, usize, usize),
    ) -> Token<'source> {
        if self.current_offset == digits_start {
            error_missing_digits(self, start_offset, 2);
        }

        self.check_trailing_underscore();

        if self.current_byte() == b'i' && !self.peek_byte().is_ascii_alphanumeric() {
            self.next();
            let end_offset = self.current_offset;
            self.error_non_decimal_imaginary(base, start_offset, end_offset - start_offset);
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

    fn lex_hex_number(&mut self, start_offset: usize) -> Token<'source> {
        let digits_start = self.current_offset;

        self.scan_digits(|byte| byte.is_ascii_hexdigit());

        self.finish_radix_number(
            start_offset,
            digits_start,
            "hex",
            Self::error_missing_hex_digits,
        )
    }

    fn lex_octal_number(&mut self, start_offset: usize) -> Token<'source> {
        let digits_start = self.current_offset;

        loop {
            self.scan_digits(|byte| (b'0'..=b'7').contains(&byte));
            if matches!(self.current_byte(), b'8' | b'9') {
                self.error_invalid_octal_digit(self.current_offset);
                self.next();
            } else {
                break;
            }
        }

        self.finish_radix_number(
            start_offset,
            digits_start,
            "octal",
            Self::error_missing_octal_digits,
        )
    }

    fn lex_binary_number(&mut self, start_offset: usize) -> Token<'source> {
        let digits_start = self.current_offset;

        loop {
            self.scan_digits(|byte| byte == b'0' || byte == b'1');
            if (b'2'..=b'9').contains(&self.current_byte()) {
                self.error_invalid_binary_digit(self.current_offset);
                self.next();
            } else {
                break;
            }
        }

        self.finish_radix_number(
            start_offset,
            digits_start,
            "binary",
            Self::error_missing_binary_digits,
        )
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

    fn consume_escape(&mut self, literal_start: usize, closing_quote: u8) -> bool {
        let escape_start = self.current_offset;
        self.next();

        if self.at_eof() {
            self.error_unterminated_escape(literal_start);
            return false;
        }

        match self.current_byte() {
            first @ b'0'..=b'7' => {
                self.next();
                let value = self.consume_octal_escape(first);
                if value > 255 {
                    self.error_octal_escape_out_of_range(
                        escape_start,
                        self.current_offset - escape_start,
                    );
                    return false;
                }
                true
            }
            b'x' => {
                self.next();
                self.consume_hex_escape(escape_start, 2).is_some()
            }
            b'u' => {
                self.next();
                match self.consume_unicode_escape(escape_start) {
                    Some(codepoint) => self.check_scalar_value(codepoint, escape_start),
                    None => false,
                }
            }
            b'U' => {
                self.next();
                match self.consume_hex_escape(escape_start, 8) {
                    Some(codepoint) => self.check_scalar_value(codepoint, escape_start),
                    None => false,
                }
            }
            b'a' | b'b' | b'f' | b'n' | b'r' | b't' | b'v' | b'\\' => {
                self.next();
                true
            }
            byte if byte == closing_quote => {
                self.next();
                true
            }
            _ => {
                self.error_invalid_escape(self.current_char(), escape_start, closing_quote);
                self.next();
                false
            }
        }
    }

    fn check_scalar_value(&mut self, codepoint: u32, escape_start: usize) -> bool {
        if char::from_u32(codepoint).is_some() {
            return true;
        }
        self.error_unicode_escape_out_of_range(escape_start, self.current_offset - escape_start);
        false
    }

    fn consume_hex_escape(&mut self, escape_start: usize, digits: usize) -> Option<u32> {
        let mut value: u32 = 0;
        for _ in 0..digits {
            // At end of input `current_byte` yields 0, which is not a hex digit.
            let Some(digit) = (self.current_byte() as char).to_digit(16) else {
                self.error_invalid_hex_escape(
                    escape_start,
                    self.current_offset - escape_start,
                    digits,
                );
                return None;
            };
            value = value * 16 + digit;
            self.next();
        }
        Some(value)
    }

    fn consume_unicode_escape(&mut self, escape_start: usize) -> Option<u32> {
        if self.at_eof() || self.current_byte() != b'{' {
            self.error_invalid_unicode_escape(escape_start, self.current_offset - escape_start);
            return None;
        }
        self.next();

        let hex_start = self.current_offset;
        let mut all_hex = true;
        while !self.at_eof() {
            let byte = self.current_byte();
            if byte == b'}' || byte == b'"' || byte == b'\'' || byte == b'\n' {
                break;
            }
            if !byte.is_ascii_hexdigit() {
                all_hex = false;
            }
            self.next();
        }
        let hex_end = self.current_offset;

        let closed = !self.at_eof() && self.current_byte() == b'}';
        if closed {
            self.next();
        }

        let hex_len = hex_end - hex_start;

        if !closed || !all_hex || hex_len == 0 || hex_len > 6 {
            self.error_invalid_unicode_escape(escape_start, self.current_offset - escape_start);
            return None;
        }

        Some(
            u32::from_str_radix(&self.input[hex_start..hex_end], 16)
                .expect("hex digits validated above"),
        )
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

        let mut terminated = false;

        while !self.at_eof() {
            let byte = self.current_byte();
            if byte == b'\\' {
                self.consume_escape(start_offset, b'"');
                continue;
            }
            if byte == b'"' {
                terminated = true;
                self.next();
                break;
            }
            self.next();
        }

        let end_offset = self.current_offset;
        let length = end_offset - start_offset;

        if !terminated {
            self.error_unterminated_string(start_offset, 1);
        }

        Token {
            kind: TokenKind::String,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: length as u32,
        }
    }

    fn lex_raw_string_literal(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;
        self.next(); // consume 'r'
        self.next(); // consume opening '"'

        let mut terminated = false;
        while !self.at_eof() {
            let byte = self.current_byte();
            if byte == b'"' {
                terminated = true;
                self.next();
                break;
            } else if byte == 0 {
                self.error_disallowed_byte_in_raw_string(self.current_offset, byte);
                self.next();
                continue;
            }
            self.next();
        }

        let end_offset = self.current_offset;
        let length = end_offset - start_offset;

        if !terminated {
            self.error_unterminated_raw_string(start_offset, 2);
        }

        Token {
            kind: TokenKind::RawString,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: length as u32,
        }
    }

    fn try_consume_unsupported_raw_variant(&mut self, end: usize) -> bool {
        let raw_format_prefix = if self.current_byte() == b'r'
            && self.peek_byte() == b'f'
            && self.peek_byte_at(2) == b'"'
        {
            Some("rf")
        } else if self.current_byte() == b'f'
            && self.peek_byte() == b'r'
            && self.peek_byte_at(2) == b'"'
        {
            Some("fr")
        } else {
            None
        };
        if let Some(prefix) = raw_format_prefix {
            let start = self.current_offset;
            self.skip(3);
            while self.current_offset < end
                && self.current_byte() != b'"'
                && self.current_byte() != b'\n'
            {
                self.next();
            }
            if self.current_offset < end && self.current_byte() == b'"' {
                self.next();
            }
            let length = self.current_offset - start;
            self.error_unsupported_raw_format_string(start, length, prefix);
            return true;
        }

        if self.current_byte() == b'r' && self.peek_byte() == b'#' {
            let mut hash_count = 0usize;
            let mut probe = self.current_offset + 1;
            while probe < self.input.len() && self.input.as_bytes()[probe] == b'#' {
                hash_count += 1;
                probe += 1;
            }
            if hash_count > 0 && probe < self.input.len() && self.input.as_bytes()[probe] == b'"' {
                let start = self.current_offset;
                self.skip(1 + hash_count + 1);
                loop {
                    if self.current_offset >= end || self.current_byte() == b'\n' {
                        break;
                    }
                    if self.current_byte() == b'"' {
                        let mut closer_matches = true;
                        for i in 1..=hash_count {
                            if self.peek_byte_at(i) != b'#' {
                                closer_matches = false;
                                break;
                            }
                        }
                        if closer_matches {
                            self.skip(1 + hash_count);
                            break;
                        }
                    }
                    self.next();
                }
                let length = self.current_offset - start;
                self.error_unsupported_hash_delimited_raw_string(start, length);
                return true;
            }
        }

        false
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

            if self.try_consume_unsupported_raw_variant(interpolation_end) {
                continue;
            }

            if self.current_byte() == b'f' && self.peek_byte() == b'"' {
                let mut fstring_tokens = self.lex_format_string_tokens();
                tokens.append(&mut fstring_tokens);
            } else if self.current_byte() == b'\\' && self.peek_byte() == b'"' {
                self.error_escaped_quote_in_interpolation(self.current_offset);
                self.skip(2);
            } else if self.current_byte() == b'r' && self.peek_byte() == b'"' {
                self.error_raw_string_in_interpolation(self.current_offset);
                self.skip(2);
                while self.current_offset < interpolation_end
                    && self.current_byte() != b'"'
                    && self.current_byte() != b'\n'
                {
                    self.next();
                }
                if self.current_offset < interpolation_end && self.current_byte() == b'"' {
                    self.next();
                }
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

    // Caller has just consumed `{` of the broken interpolation, so we start
    // inside it (depth=1). Newlines are not a recovery boundary now that
    // f-string text spans them, so we balance braces and skip past quoted
    // strings to avoid stopping at the first inner `"`.
    fn skip_to_format_string_end(&mut self) {
        let mut depth = 1;
        while !self.at_eof() {
            match self.current_byte() {
                b'\\' => {
                    self.next();
                    if !self.at_eof() {
                        self.next();
                    }
                }
                b'"' if depth == 0 => {
                    self.next();
                    return;
                }
                b'"' => {
                    self.next();
                    while !self.at_eof() && self.current_byte() != b'"' {
                        if self.current_byte() == b'\\' {
                            self.next();
                            if self.at_eof() {
                                break;
                            }
                        }
                        self.next();
                    }
                    if !self.at_eof() {
                        self.next();
                    }
                }
                b'{' => {
                    depth += 1;
                    self.next();
                }
                b'}' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    self.next();
                }
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
                b'\\' => {
                    self.consume_escape(start_offset, b'"');
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

        self.error_unterminated_format_string(start_offset, 2);
        tokens
    }

    fn lex_char(&mut self) -> Token<'source> {
        let start_offset = self.current_offset;

        self.next();
        self.scan_rune_body(start_offset);

        let end_offset = self.current_offset;
        Token {
            kind: TokenKind::Char,
            text: &self.input[start_offset..end_offset],
            byte_offset: start_offset as u32,
            byte_length: (end_offset - start_offset) as u32,
        }
    }

    fn scan_rune_body(&mut self, start_offset: usize) {
        if self.at_eof() || self.current_byte() == b'\'' {
            self.error_empty_rune_literal(start_offset);
            return;
        }

        if self.current_byte() != b'\\' {
            self.next();
        } else if !self.consume_escape(start_offset, b'\'') {
            // Resync, else a mid-literal cursor also reports unterminated_rune.
            while !self.at_eof() && self.current_byte() != b'\'' {
                self.next();
            }
            if !self.at_eof() {
                self.next();
            }
            return;
        }

        if !self.at_eof() && self.current_byte() == b'\'' {
            self.next();
        } else {
            self.error_unterminated_rune(start_offset, self.current_offset - start_offset);
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

        if slash_count == 2 && self.current_byte() == b'!' {
            self.next();
            if self.current_byte() == b' ' {
                self.next();
            }
            let text_start = self.current_offset;
            self.skip_to_eol();
            let end_offset = self.current_offset;

            return Token {
                kind: TokenKind::FileComment,
                text: &self.input[text_start..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        if slash_count == 3 {
            if self.current_byte() == b' ' {
                self.next();
            }
            let text_start = self.current_offset;
            self.skip_to_eol();
            let end_offset = self.current_offset;

            return Token {
                kind: TokenKind::DocComment,
                text: &self.input[text_start..end_offset],
                byte_offset: start_offset as u32,
                byte_length: (end_offset - start_offset) as u32,
            };
        }

        self.skip_to_eol();
        let end_offset = self.current_offset;

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
        while offset < self.input.len() && self.input.as_bytes()[offset] == byte {
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

/// Decodes a quote-stripped rune literal, covering every escape the lexer takes.
pub fn rune_codepoint(text: &str) -> Option<u32> {
    let Some(rest) = text.strip_prefix('\\') else {
        return text.chars().next().map(|c| c as u32);
    };
    match rest.as_bytes().first()? {
        b'a' => Some(7),
        b'b' => Some(8),
        b'f' => Some(12),
        b'n' => Some(10),
        b'r' => Some(13),
        b't' => Some(9),
        b'v' => Some(11),
        b'\\' => Some(92),
        b'\'' => Some(39),
        b'x' | b'U' => u32::from_str_radix(&rest[1..], 16).ok(),
        b'u' => {
            let braced = rest[1..].strip_prefix('{')?.strip_suffix('}')?;
            u32::from_str_radix(braced, 16).ok()
        }
        b'0'..=b'7' => u32::from_str_radix(rest, 8).ok(),
        _ => None,
    }
}

pub fn interpolation_holes(value: &str) -> Option<Vec<&str>> {
    let bytes = value.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;

    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at = skip_escape(value, at)?,
            b'{' if bytes.get(at + 1) == Some(&b'{') => return None,
            b'{' => {
                let start = at + 1;
                let close = start + value[start..].find('}')?;
                let name = &value[start..close];
                if !is_bare_identifier(name) {
                    return None;
                }
                names.push(name);
                at = close + 1;
            }
            b'}' => return None,
            _ => at += 1,
        }
    }

    (!names.is_empty()).then_some(names)
}

fn skip_escape(value: &str, at: usize) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.get(at + 1) == Some(&b'u') && bytes.get(at + 2) == Some(&b'{') {
        return Some(at + 4 + value[at + 3..].find('}')?);
    }
    let escaped = value[at + 1..].chars().next()?;
    Some(at + 1 + escaped.len_utf8())
}

fn is_bare_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Decodes a quote-stripped string literal to the bytes it holds at runtime,
/// covering every escape the lexer takes. `None` for a malformed escape.
pub fn string_bytes(text: &str, raw: bool) -> Option<Cow<'_, [u8]>> {
    if raw || !text.contains('\\') {
        return Some(Cow::Borrowed(text.as_bytes()));
    }

    let mut decoded = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            push_char(&mut decoded, ch);
            continue;
        }
        match chars.next()? {
            'a' => decoded.push(0x07),
            'b' => decoded.push(0x08),
            'f' => decoded.push(0x0c),
            'n' => decoded.push(b'\n'),
            'r' => decoded.push(b'\r'),
            't' => decoded.push(b'\t'),
            'v' => decoded.push(0x0b),
            '\\' => decoded.push(b'\\'),
            '"' => decoded.push(b'"'),
            '\'' => decoded.push(b'\''),
            'x' => decoded.push(byte_from_hex(&mut chars)?),
            'u' => push_char(&mut decoded, char_from_braced_hex(&mut chars)?),
            'U' => push_char(&mut decoded, char_from_eight_hex(&mut chars)?),
            first @ '0'..='7' => decoded.push(byte_from_octal(&mut chars, first)?),
            _ => return None,
        }
    }

    Some(Cow::Owned(decoded))
}

fn push_char(decoded: &mut Vec<u8>, ch: char) {
    let mut buffer = [0u8; 4];
    decoded.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
}

fn byte_from_hex(chars: &mut Peekable<Chars<'_>>) -> Option<u8> {
    let high = chars.next()?.to_digit(16)?;
    let low = chars.next()?.to_digit(16)?;
    Some((high * 16 + low) as u8)
}

fn byte_from_octal(chars: &mut Peekable<Chars<'_>>, first: char) -> Option<u8> {
    let mut value = first.to_digit(8)?;
    for _ in 0..2 {
        let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(8)) else {
            break;
        };
        chars.next();
        value = value * 8 + digit;
    }
    u8::try_from(value).ok()
}

fn char_from_eight_hex(chars: &mut Peekable<Chars<'_>>) -> Option<char> {
    let mut value = 0u32;
    for _ in 0..8 {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    char::from_u32(value)
}

fn char_from_braced_hex(chars: &mut Peekable<Chars<'_>>) -> Option<char> {
    if chars.next()? != '{' {
        return None;
    }
    let mut value = 0u32;
    let mut digits = 0;
    while let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(16)) {
        chars.next();
        value = value * 16 + digit;
        digits += 1;
        if digits > 6 {
            return None;
        }
    }
    if digits == 0 || chars.next()? != '}' {
        return None;
    }
    char::from_u32(value)
}

#[cfg(test)]
mod tests {
    use super::{Lexer, rune_codepoint, string_bytes};

    #[test]
    fn rune_codepoint_decodes_every_escape_the_lexer_accepts() {
        let cases = [
            ("'a'", 97),
            ("'中'", 0x4E2D),
            ("'\\a'", 7),
            ("'\\b'", 8),
            ("'\\f'", 12),
            ("'\\n'", 10),
            ("'\\r'", 13),
            ("'\\t'", 9),
            ("'\\v'", 11),
            ("'\\\\'", 92),
            ("'\\''", 39),
            ("'\\x41'", 65),
            ("'\\xFF'", 255),
            ("'\\101'", 65),
            ("'\\377'", 255),
            ("'\\u{41}'", 65),
            ("'\\u{e9}'", 233),
            ("'\\U0001F600'", 0x1F600),
        ];

        for (source, expected) in cases {
            let result = Lexer::new(source, 0).lex();
            assert!(result.errors.is_empty(), "{source} should lex cleanly");
            let inner = &source[1..source.len() - 1];
            assert_eq!(rune_codepoint(inner), Some(expected), "{source}");
        }
    }

    #[test]
    fn string_bytes_decodes_every_escape_the_lexer_accepts() {
        let cases: [(&str, &[u8]); 17] = [
            ("\"ab\"", b"ab"),
            ("\"\\a\"", &[7]),
            ("\"\\b\"", &[8]),
            ("\"\\f\"", &[12]),
            ("\"\\n\"", b"\n"),
            ("\"\\r\"", b"\r"),
            ("\"\\t\"", b"\t"),
            ("\"\\v\"", &[11]),
            ("\"\\\\\"", b"\\"),
            ("\"\\\"\"", b"\""),
            ("\"\\x41\"", b"A"),
            ("\"\\xff\"", &[255]),
            ("\"\\101\"", b"A"),
            ("\"\\377\"", &[255]),
            ("\"\\u{41}\"", b"A"),
            ("\"\\u{e9}\"", "é".as_bytes()),
            ("\"\\U0001F600\"", "😀".as_bytes()),
        ];

        for (source, expected) in cases {
            let result = Lexer::new(source, 0).lex();
            assert!(result.errors.is_empty(), "{source} should lex cleanly");
            let inner = &source[1..source.len() - 1];
            assert_eq!(
                string_bytes(inner, false).as_deref(),
                Some(expected),
                "{source}"
            );
        }
    }

    #[test]
    fn string_bytes_keeps_raw_backslashes() {
        assert_eq!(string_bytes("a\\nb", true).as_deref(), Some(&b"a\\nb"[..]));
    }

    #[test]
    fn string_bytes_rejects_malformed_escapes() {
        assert!(string_bytes("\\q", false).is_none());
        assert!(string_bytes("\\x4", false).is_none());
        assert!(string_bytes("\\u0041", false).is_none());
        assert!(string_bytes("\\400", false).is_none());
    }
}
