use crate::ast::Span;
use crate::parse::ParseError;

use super::Lexer;

impl<'source> Lexer<'source> {
    fn span(&self, offset: u32, length: u32) -> Span {
        Span::new(self.file_id, offset, length)
    }

    pub(super) fn error_consecutive_underscores(&mut self, start_byte_offset: usize) {
        let mut count = 1;
        let mut offset = start_byte_offset + 1;
        while offset < self.input.len() {
            match self.input[offset..].chars().next() {
                Some('_') => {
                    count += 1;
                    offset += 1;
                }
                _ => break,
            }
        }

        let span = self.span(start_byte_offset as u32, count as u32);
        let error = ParseError::new("Invalid number literal", span, "consecutive underscores")
            .with_lex_code("number_consecutive_underscores")
            .with_help("Use a single underscore to separate each digit group");
        self.errors.push(error);
    }

    pub(super) fn error_number_trailing_underscore(&mut self, offset: usize) {
        let span = self.span(offset as u32, 1);
        let error = ParseError::new("Invalid number literal", span, "trailing underscore")
            .with_lex_code("number_trailing_underscore")
            .with_help("Remove the trailing underscore");
        self.errors.push(error);
    }

    pub(super) fn error_decimal_leading_underscore(&mut self, offset: usize) {
        let span = self.span(offset as u32, 1);
        let error = ParseError::new("Invalid number literal", span, "leading underscore")
            .with_lex_code("number_decimal_leading_underscore")
            .with_help("Remove the underscore after the decimal point");
        self.errors.push(error);
    }

    pub(super) fn error_missing_exponent_digits(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Invalid number literal",
            span,
            "expected digits after exponent",
        )
        .with_lex_code("number_missing_exponent")
        .with_help("Add digits after `e`, e.g. `1e10` or `1.5e-3`");
        self.errors.push(error);
    }

    pub(super) fn error_missing_hex_digits(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new("Invalid hex literal", span, "expected digits after `0x`")
            .with_lex_code("number_missing_hex_digits")
            .with_help("Add hex digits after `0x`, e.g. `0xFF` or `0x1A2B`");
        self.errors.push(error);
    }

    pub(super) fn error_missing_octal_digits(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new("Invalid octal literal", span, "expected digits after `0o`")
            .with_lex_code("number_missing_octal_digits")
            .with_help("Add octal digits after `0o`, e.g. `0o755` or `0o644`");
        self.errors.push(error);
    }

    pub(super) fn error_invalid_octal_digit(&mut self, offset: usize) {
        let digit = self.current_char();
        let span = self.span(offset as u32, 1);
        let error = ParseError::new(
            "Invalid octal literal",
            span,
            format!("`{}` is not a valid octal digit", digit),
        )
        .with_lex_code("number_invalid_octal_digit")
        .with_help("Use digits `0` to `7` for octal literals");
        self.errors.push(error);
    }

    pub(super) fn error_missing_binary_digits(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new("Invalid binary literal", span, "expected digits after `0b`")
            .with_lex_code("number_missing_binary_digits")
            .with_help("Add binary digits after `0b`, e.g. `0b1010` or `0b1111_0000`");
        self.errors.push(error);
    }

    pub(super) fn error_invalid_binary_digit(&mut self, offset: usize) {
        let digit = self.current_char();
        let span = self.span(offset as u32, 1);
        let error = ParseError::new(
            "Invalid binary literal",
            span,
            format!("`{}` is not a valid binary digit", digit),
        )
        .with_lex_code("number_invalid_binary_digit")
        .with_help("Use only `0` and `1` for binary literals");
        self.errors.push(error);
    }

    pub(super) fn error_unterminated_string(&mut self, start_offset: usize, length: usize) {
        let span = self.span(start_offset as u32, length as u32);
        let error = ParseError::new("Unterminated string literal", span, "string not completed")
            .with_lex_code("unterminated_string")
            .with_help("Add a closing double quote");
        self.errors.push(error);
    }

    pub(super) fn error_unterminated_raw_string(&mut self, start_offset: usize, length: usize) {
        let span = self.span(start_offset as u32, length as u32);
        let error = ParseError::new(
            "Unterminated raw string literal",
            span,
            "raw string not closed",
        )
        .with_lex_code("unterminated_raw_string")
        .with_help("Add a closing double quote");
        self.errors.push(error);
    }

    pub(super) fn error_raw_string_in_interpolation(&mut self, offset: usize) {
        let span = self.span(offset as u32, 2);
        let error = ParseError::new(
            "Raw string in f-string interpolation",
            span,
            "raw strings are not allowed inside `{...}`",
        )
        .with_lex_code("raw_string_in_interpolation")
        .with_help("Extract the raw string into a `let` binding above the f-string");
        self.errors.push(error);
    }

    pub(super) fn error_unsupported_raw_format_string(
        &mut self,
        offset: usize,
        length: usize,
        prefix: &str,
    ) {
        let span = self.span(offset as u32, length as u32);
        let help = if prefix == "fr" {
            "Raw format strings are not yet implemented. When supported, the canonical spelling will be `rf\"...\"`. For now, use a regular f-string with escaped backslashes"
        } else {
            "Raw format strings are not yet implemented. For now, use a regular f-string with escaped backslashes"
        };
        let error = ParseError::new(
            "Raw format strings are currently not supported",
            span,
            "not yet implemented",
        )
        .with_lex_code("unsupported_raw_format_string")
        .with_help(help);
        self.errors.push(error);
    }

    pub(super) fn error_unsupported_hash_delimited_raw_string(
        &mut self,
        offset: usize,
        length: usize,
    ) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Hash-delimited raw strings are not supported",
            span,
            "not yet implemented",
        )
        .with_lex_code("unsupported_hash_delimited_raw_string")
        .with_help(
            "Hash-delimited raw strings (`r#\"...\"#`) are not yet implemented. For now, use a regular string with escaped quotes",
        );
        self.errors.push(error);
    }

    pub(super) fn error_disallowed_byte_in_raw_string(&mut self, offset: usize, byte: u8) {
        let span = self.span(offset as u32, 1);
        let (label, help) = match byte {
            0 => (
                "NUL byte is not allowed in string literals",
                "Go source cannot contain NUL. Remove this byte",
            ),
            _ => (
                "byte is not allowed in raw string content",
                "Remove this byte or use a non-raw string literal",
            ),
        };
        let error = ParseError::new("Disallowed byte in raw string", span, label)
            .with_lex_code("disallowed_byte_in_raw_string")
            .with_help(help);
        self.errors.push(error);
    }

    pub(super) fn error_unterminated_format_string(&mut self, start_offset: usize, length: usize) {
        let span = self.span(start_offset as u32, length as u32);
        let error = ParseError::new(
            "Unterminated format string",
            span,
            "format string not completed",
        )
        .with_lex_code("format_string_unterminated")
        .with_help("Add a closing double quote");
        self.errors.push(error);
    }

    pub(super) fn error_unclosed_brace_in_format_string(&mut self, offset: usize) {
        let span = self.span(offset as u32, 1);
        let error = ParseError::new("Unclosed `{` in format string", span, "unclosed brace")
            .with_lex_code("format_string_unclosed_brace")
            .with_help("Add a closing `}` or escape it with `{{`");
        self.errors.push(error);
    }

    pub(super) fn error_escaped_quote_in_interpolation(&mut self, offset: usize) {
        let span = self.span(offset as u32, 2);
        let error = ParseError::new("Escaped quote in f-string interpolation", span, "unneeded")
            .with_lex_code("escaped_quote_in_interpolation")
            .with_help("Use bare quotes inside f-string interpolations: `f\"x: {func(\"arg\")}\"`");
        self.errors.push(error);
    }

    pub(super) fn error_multiline_format_string_interpolation(&mut self, offset: usize) {
        let span = self.span(offset as u32, 1);
        let error = ParseError::new(
            "Multi-line f-string interpolation",
            span,
            "f-string interpolations must be single-line",
        )
        .with_lex_code("format_string_multiline_interpolation")
        .with_help("Extract the expression into a `let` binding and interpolate the variable");
        self.errors.push(error);
    }

    pub(super) fn error_unmatched_brace_in_format_string(&mut self, offset: usize) {
        let span = self.span(offset as u32, 1);
        let error = ParseError::new(
            "Unmatched `}` in format string",
            span,
            "unmatched closing brace",
        )
        .with_lex_code("format_string_unmatched_brace")
        .with_help("Remove `}` or escape it with `}}`");
        self.errors.push(error);
    }

    pub(super) fn error_empty_rune_literal(&mut self, offset: usize) {
        let span = self.span(offset as u32, 2);
        let error = ParseError::new("Empty rune literal", span, "empty rune")
            .with_lex_code("empty_rune")
            .with_help("Add a rune between the single quotes");
        self.errors.push(error);
    }

    pub(super) fn error_unterminated_escape(&mut self, offset: usize) {
        let span = self.span(offset as u32, 1);
        let error = ParseError::new(
            "Unterminated escape sequence",
            span,
            "escape sequence not completed",
        )
        .with_lex_code("unterminated_escape")
        .with_help("Complete the escape sequence or remove the backslash");
        self.errors.push(error);
    }

    pub(super) fn error_invalid_escape(&mut self, ch: char) {
        let span = self.span((self.current_offset - 1) as u32, 2);
        let error = ParseError::new(
            "Invalid escape sequence",
            span,
            format!("`\\{ch}` is not a valid escape"),
        )
        .with_lex_code("invalid_escape_sequence")
        .with_help(
            "Valid escapes are `\\a`, `\\b`, `\\f`, `\\n`, `\\r`, `\\t`, `\\v`, `\\\\`, `\\'`, `\\xHH` (hex), `\\ooo` (octal), and `\\u{HEX}` (unicode)",
        );
        self.errors.push(error);
    }

    pub(super) fn error_octal_escape_out_of_range(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Octal escape out of range",
            span,
            "octal escape value exceeds maximum (0o377 / 0xFF)",
        )
        .with_lex_code("octal_escape_out_of_range")
        .with_help("Octal escapes must be in the range `\\0` to `\\377` (0x00 to 0xFF)");
        self.errors.push(error);
    }

    pub(super) fn error_invalid_unicode_escape(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Invalid unicode escape",
            span,
            "expected `\\u{HEX}` with 1-6 hex digits",
        )
        .with_lex_code("invalid_unicode_escape")
        .with_help("Use the form `\\u{1F600}` with 1-6 hexadecimal digits between braces");
        self.errors.push(error);
    }

    pub(super) fn error_unicode_escape_out_of_range(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Unicode escape out of range",
            span,
            "codepoint exceeds U+10FFFF or is a surrogate (U+D800-U+DFFF)",
        )
        .with_lex_code("unicode_escape_out_of_range")
        .with_help("Unicode escapes must be valid scalar values: 0..=0xD7FF or 0xE000..=0x10FFFF");
        self.errors.push(error);
    }

    pub(super) fn error_unterminated_rune(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new("Unterminated rune literal", span, "rune literal not closed")
            .with_lex_code("unterminated_rune")
            .with_help("Add a closing single quote");
        self.errors.push(error);
    }

    pub(super) fn error_unterminated_backtick(&mut self, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Unterminated backtick literal",
            span,
            "backtick literal not closed",
        )
        .with_lex_code("unterminated_backtick")
        .with_help("Add a closing backtick");
        self.errors.push(error);
    }

    pub(super) fn error_unexpected_char(&mut self, offset: usize, ch: char) {
        let help = "Remove this character";

        let span = self.span(offset as u32, ch.len_utf8() as u32);
        let error = ParseError::new("Unexpected character", span, "unexpected character")
            .with_lex_code("unexpected_char")
            .with_help(help);
        self.errors.push(error);
    }

    pub(super) fn error_excess_slashes_in_comment(&mut self, offset: usize, count: usize) {
        let span = self.span(offset as u32, count as u32);
        let error = ParseError::new("Invalid comment", span, "expected 2 or 3 slashes")
            .with_lex_code("excess_slashes_in_comment")
            .with_help("Use `//` for regular comments or `///` for doc comments");
        self.errors.push(error);
    }

    pub(super) fn error_non_decimal_imaginary(&mut self, base: &str, offset: usize, length: usize) {
        let span = self.span(offset as u32, length as u32);
        let error = ParseError::new(
            "Invalid imaginary literal",
            span,
            format!("{} imaginary literals are not supported", base),
        )
        .with_lex_code("non_decimal_imaginary")
        .with_help("Use decimal notation for imaginary literals, e.g. `16i` instead of `0x10i`");
        self.errors.push(error);
    }
}
