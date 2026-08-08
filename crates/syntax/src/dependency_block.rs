//! Finds a script's `[dependencies.go]` table, written as `//` comments the
//! lexer never sees. Line offsets are kept, for mapping a TOML error back.

use crate::ast::Span;

pub const HEADER: &str = "[dependencies.go]";

#[derive(Debug, Clone)]
pub struct DependencyBlock {
    pub text: String,
    line_starts: Vec<u32>,
    pub span: Span,
}

impl DependencyBlock {
    pub fn map_span(&self, range: std::ops::Range<usize>, file_id: u32) -> Span {
        let Some(line) = self.line_of(range.start) else {
            return self.span;
        };
        let column = (range.start - self.line_offset(line)) as u32;
        let line_end = self.text[range.start..]
            .find('\n')
            .map_or(self.text.len(), |offset| range.start + offset);
        let length = (range.end.min(line_end).saturating_sub(range.start)).max(1) as u32;
        Span::new(file_id, self.line_starts[line] + column, length)
    }

    fn line_of(&self, byte: usize) -> Option<usize> {
        if byte > self.text.len() {
            return None;
        }
        let count = self.text[..byte].bytes().filter(|&b| b == b'\n').count();
        (count < self.line_starts.len()).then_some(count)
    }

    fn line_offset(&self, line: usize) -> usize {
        self.text
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum()
    }
}

pub fn scan_dependency_blocks(source: &str, file_id: u32) -> Vec<DependencyBlock> {
    let mut blocks = Vec::new();
    let mut offset = prologue_start(source);

    while offset < source.len() {
        let line = line_at(source, offset);
        let trimmed = line.trim_end_matches(['\r', '\n']);

        if trimmed.trim().is_empty() {
            offset += line.len();
            continue;
        }
        if trimmed.starts_with("//!") {
            offset += line.len();
            continue;
        }
        if !trimmed.starts_with("//") {
            break;
        }
        match comment_content(trimmed).map(str::trim) {
            Some(HEADER) => {
                let block = take_block(source, offset, file_id);
                offset = block.span.byte_offset as usize + block.span.byte_length as usize;
                blocks.push(block);
            }
            _ => offset += line.len(),
        }
    }
    blocks
}

/// Minus the prefix and at most one space. A wider indent TOML tolerates.
fn comment_content(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("//")?;
    if rest.starts_with("!") {
        return None;
    }
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// From the header to the first line that is not a plain `//` comment.
fn take_block(source: &str, start: usize, file_id: u32) -> DependencyBlock {
    let mut text = String::new();
    let mut line_starts = Vec::new();
    let mut offset = start;

    while offset < source.len() {
        let line = line_at(source, offset);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let Some(content) = comment_content(trimmed) else {
            break;
        };

        let prefix = trimmed.len() - content.len();
        line_starts.push((offset + prefix) as u32);
        text.push_str(content);
        text.push('\n');
        offset += line.len();
    }

    DependencyBlock {
        text,
        line_starts,
        span: Span::new(file_id, start as u32, (offset - start) as u32),
    }
}

fn line_at(source: &str, offset: usize) -> &str {
    let rest = &source[offset..];
    match rest.find('\n') {
        Some(end) => &rest[..end + 1],
        None => rest,
    }
}

/// After a shebang and any file doc comments, before the first import or item.
pub fn insertion_point(source: &str) -> usize {
    let mut offset = prologue_start(source);
    offset += line_break_len(&source[offset..]);
    if offset > 0 {
        offset += line_break_len(&source[offset..]);
    }
    while source[offset..].starts_with("//!") {
        offset += line_at(source, offset).len();
    }
    offset
}

fn line_break_len(rest: &str) -> usize {
    if rest.starts_with("\r\n") {
        2
    } else {
        usize::from(rest.starts_with('\n'))
    }
}

fn prologue_start(source: &str) -> usize {
    let bom = crate::lex::bom_len(source);
    bom + crate::lex::shebang_len(&source[bom..]).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(source: &str) -> DependencyBlock {
        let mut blocks = scan_dependency_blocks(source, 0);
        assert_eq!(blocks.len(), 1, "expected one block in {source:?}");
        blocks.remove(0)
    }

    #[test]
    fn reads_a_table_after_a_shebang() {
        let source = "#!/usr/bin/env lis\n\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\nimport \"go:fmt\"\n";
        let block = only(source);

        assert_eq!(block.text, "[dependencies.go]\n\"x.y/z\" = \"v1.0.0\"\n");
    }

    #[test]
    fn the_header_is_recognised_whatever_surrounds_it() {
        for header in [
            "// [dependencies.go]",
            "//[dependencies.go]",
            "//   [dependencies.go]",
            "// [dependencies.go]   ",
            "//\t[dependencies.go]\t",
        ] {
            let source = format!("{}\n// \"x.y/z\" = \"v1.0.0\"\n", header);
            let blocks = scan_dependency_blocks(&source, 0);

            assert_eq!(blocks.len(), 1, "not found in {header:?}");
            assert!(blocks[0].text.contains("x.y/z"), "empty in {header:?}");
        }
    }

    #[test]
    fn reads_a_table_with_no_shebang_and_no_leading_space() {
        let source = "//[dependencies.go]\n//\"x.y/z\" = \"v1.0.0\"\n";
        let block = only(source);

        assert_eq!(block.text, "[dependencies.go]\n\"x.y/z\" = \"v1.0.0\"\n");
    }

    #[test]
    fn a_blank_line_ends_the_block() {
        let source =
            "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\n// \"x.y/w\" = \"v2.0.0\"\n";
        let block = only(source);

        assert!(!block.text.contains("x.y/w"));
    }

    #[test]
    fn a_file_doc_comment_does_not_open_or_extend_a_block() {
        let source =
            "//! A script.\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n//! trailing\n";
        let block = only(source);

        assert_eq!(block.text, "[dependencies.go]\n\"x.y/z\" = \"v1.0.0\"\n");
    }

    #[test]
    fn a_comment_after_code_is_not_a_block() {
        let source = "import \"go:fmt\"\n\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n";

        assert!(scan_dependency_blocks(source, 0).is_empty());
    }

    #[test]
    fn two_blocks_are_both_returned() {
        let source = "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\n// [dependencies.go]\n// \"x.y/w\" = \"v2.0.0\"\n";

        assert_eq!(scan_dependency_blocks(source, 0).len(), 2);
    }

    #[test]
    fn maps_a_toml_range_back_to_the_source() {
        let source = "#!/usr/bin/env lis\n\n// [dependencies.go]\n//   \"x.y/z\" = \"bad\"\n";
        let block = only(source);
        let value = block.text.find("\"bad\"").unwrap();
        let span = block.map_span(value..value + 5, 0);

        let start = span.byte_offset as usize;
        assert_eq!(&source[start..start + span.byte_length as usize], "\"bad\"");
    }

    #[test]
    fn a_range_crossing_a_line_break_stops_at_the_break() {
        let source = "// [dependencies.go]\n// \"x.y/z\" = [\n//   1, 2,\n";
        let block = only(source);
        let entry = block.text.find("\"x.y/z\"").unwrap();
        let span = block.map_span(entry..block.text.len(), 0);

        let start = span.byte_offset as usize;
        let underlined = &source[start..start + span.byte_length as usize];
        assert_eq!(underlined, "\"x.y/z\" = [");
    }

    #[test]
    fn the_span_covers_the_whole_block() {
        let source = "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\nimport \"go:fmt\"\n";
        let block = only(source);
        let start = block.span.byte_offset as usize;
        let text = &source[start..start + block.span.byte_length as usize];

        assert!(text.starts_with("// [dependencies.go]"));
        assert!(text.trim_end().ends_with("\"v1.0.0\""));
    }
}
