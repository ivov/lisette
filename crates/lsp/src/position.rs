use crate::protocol::{Position, Range};
use syntax::ast::Span;

pub(crate) struct LineIndex {
    line_starts: Vec<u32>,
    encoded_chars: Vec<EncodedChar>,
    source_len: u32,
}

struct EncodedChar {
    byte_start: u32,
    utf16_start: u32,
    byte_len: u8,
    utf16_len: u8,
}

impl LineIndex {
    pub(crate) fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        let mut encoded_chars = Vec::new();
        let mut utf16_offset = 0;
        for (byte_start, c) in source.char_indices() {
            let byte_len = c.len_utf8() as u8;
            let utf16_len = c.len_utf16() as u8;
            if byte_len != utf16_len {
                encoded_chars.push(EncodedChar {
                    byte_start: byte_start as u32,
                    utf16_start: utf16_offset,
                    byte_len,
                    utf16_len,
                });
            }
            utf16_offset += u32::from(utf16_len);
            if c == '\n' {
                line_starts.push(byte_start as u32 + u32::from(byte_len));
                utf16_offset = 0;
            }
        }
        Self {
            line_starts,
            encoded_chars,
            source_len: source.len() as u32,
        }
    }

    pub(crate) fn position_to_offset(&self, position: Position) -> Option<u32> {
        let line = position.line as usize;
        let line_start = *self.line_starts.get(line)?;
        let line_end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.source_len);
        let encoded_chars = self.encoded_chars_between(line_start, line_end);
        let encoded_adjustment: u32 = encoded_chars.iter().map(EncodedChar::adjustment).sum();
        let utf16_len = line_end - line_start - encoded_adjustment;
        let character = position.character.min(utf16_len);
        let mut byte_offset = character;
        for encoded in encoded_chars {
            if character <= encoded.utf16_start {
                break;
            }
            if character < encoded.utf16_start + u32::from(encoded.utf16_len) {
                return Some(encoded.byte_start + u32::from(encoded.byte_len));
            }
            byte_offset += encoded.adjustment();
        }
        Some(line_start + byte_offset)
    }

    pub(crate) fn offset_to_position(&self, offset: u32) -> Position {
        let line = self
            .line_starts
            .partition_point(|&line_start| line_start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line];
        let line_end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.source_len);
        let end = offset.min(self.source_len);
        let byte_offset = end.saturating_sub(line_start);
        let encoded_adjustment: u32 = self
            .encoded_chars_between(line_start, line_end)
            .iter()
            .take_while(|encoded| encoded.byte_start + u32::from(encoded.byte_len) <= end)
            .map(EncodedChar::adjustment)
            .sum();

        Position {
            line: line as u32,
            character: byte_offset - encoded_adjustment,
        }
    }

    fn encoded_chars_between(&self, start: u32, end: u32) -> &[EncodedChar] {
        let first = self
            .encoded_chars
            .partition_point(|encoded| encoded.byte_start < start);
        let last = self
            .encoded_chars
            .partition_point(|encoded| encoded.byte_start < end);
        &self.encoded_chars[first..last]
    }

    pub(crate) fn span_to_range(&self, span: Span) -> Range {
        Range {
            start: self.offset_to_position(span.byte_offset),
            end: self.offset_to_position(span.byte_offset + span.byte_length),
        }
    }

    pub(crate) fn offset_len_to_range(&self, offset: usize, length: usize) -> Range {
        Range {
            start: self.offset_to_position(offset as u32),
            end: self.offset_to_position((offset + length) as u32),
        }
    }
}

impl EncodedChar {
    fn adjustment(&self) -> u32 {
        u32::from(self.byte_len - self.utf16_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_to_offset_accounts_for_utf16_surrogate_pairs() {
        let index = LineIndex::new("a😀b");

        assert_eq!(index.position_to_offset(Position::new(0, 3)), Some(5));
    }

    #[test]
    fn offset_to_position_accounts_for_utf16_surrogate_pairs() {
        let index = LineIndex::new("a😀b");

        assert_eq!(index.offset_to_position(5), Position::new(0, 3));
    }

    #[test]
    fn conversions_preserve_line_boundaries_and_clamping() {
        let index = LineIndex::new("ab\nç😀z\n");

        assert_eq!(index.position_to_offset(Position::new(0, 99)), Some(3));
        assert_eq!(index.position_to_offset(Position::new(1, 1)), Some(5));
        assert_eq!(index.position_to_offset(Position::new(1, 2)), Some(9));
        assert_eq!(index.position_to_offset(Position::new(1, 4)), Some(10));
        assert_eq!(index.offset_to_position(5), Position::new(1, 1));
        assert_eq!(index.offset_to_position(9), Position::new(1, 3));
        assert_eq!(index.offset_to_position(11), Position::new(2, 0));
        assert_eq!(index.offset_to_position(99), Position::new(2, 0));
    }
}
