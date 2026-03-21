use syntax::ast::Span;
use tower_lsp::lsp_types::{Position, Range};

pub(crate) struct LineIndex {
    line_starts: Vec<u32>,
    source: String,
}

impl LineIndex {
    pub(crate) fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, c) in source.char_indices() {
            if c == '\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            line_starts,
            source: source.to_string(),
        }
    }

    pub(crate) fn position_to_offset(&self, position: Position) -> Option<u32> {
        let line_start = *self.line_starts.get(position.line as usize)?;
        let line_end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.source.len() as u32);

        let line_text = &self.source[line_start as usize..line_end as usize];

        let mut utf16_offset = 0u32;
        let mut byte_offset = 0usize;

        for c in line_text.chars() {
            if utf16_offset >= position.character {
                break;
            }
            utf16_offset += c.len_utf16() as u32;
            byte_offset += c.len_utf8();
        }

        Some(line_start + byte_offset as u32)
    }

    pub(crate) fn offset_to_position(&self, offset: u32) -> Position {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);

        let line_start = self.line_starts[line] as usize;
        let offset_usize = offset as usize;

        let end = offset_usize.min(self.source.len());
        let line_text = &self.source[line_start..end];

        let character: u32 = line_text.chars().map(|c| c.len_utf16() as u32).sum();

        Position {
            line: line as u32,
            character,
        }
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
