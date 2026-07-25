use crate::lindig::{Document, concat, join};
use syntax::lex::{Token, TokenKind};

#[derive(Debug, Clone)]
struct Comment<'a> {
    start: u32,
    content: &'a str,
}

#[derive(Debug, Clone)]
enum LineTrivia<'a> {
    Comment(Comment<'a>),
    BlankLine(u32),
}

impl LineTrivia<'_> {
    fn start(&self) -> u32 {
        match self {
            Self::Comment(comment) => comment.start,
            Self::BlankLine(start) => *start,
        }
    }
}

pub struct Comments<'a> {
    line_trivia: Vec<LineTrivia<'a>>,
    line_cursor: usize,
    doc_comments: Vec<Comment<'a>>,
    doc_comments_cursor: usize,
    file_comments: Vec<Comment<'a>>,
    source: &'a str,
}

pub(crate) struct TakenComments<'a> {
    pub(crate) document: Option<Document<'a>>,
    pub(crate) has_blank_line: bool,
}

pub(crate) struct SplitComments<'a> {
    pub(crate) trailing: Option<Document<'a>>,
    pub(crate) leading: Option<Document<'a>>,
    pub(crate) has_blank_before_leading: bool,
}

impl<'a> SplitComments<'a> {
    pub(crate) fn leading(document: Option<Document<'a>>) -> Self {
        Self {
            trailing: None,
            leading: document,
            has_blank_before_leading: false,
        }
    }
}

impl<'a> Comments<'a> {
    pub fn from_lexed(tokens: &[Token<'_>], blank_lines: Vec<u32>, source: &'a str) -> Self {
        let mut line_trivia = blank_lines
            .into_iter()
            .map(LineTrivia::BlankLine)
            .collect::<Vec<_>>();
        let mut doc_comments = Vec::new();
        let mut file_comments = Vec::new();
        for token in tokens {
            let prefix = match token.kind {
                TokenKind::Comment => "//",
                TokenKind::DocComment => "///",
                TokenKind::FileComment => "//!",
                _ => continue,
            };
            let start = token.byte_offset;
            let end = token.end_offset();
            let content = source
                .get(start as usize..end as usize)
                .expect("comment token must be a valid source range")
                .strip_prefix(prefix)
                .expect("comment token text must match its kind");
            let comment = Comment { start, content };
            match token.kind {
                TokenKind::Comment => line_trivia.push(LineTrivia::Comment(comment)),
                TokenKind::DocComment => doc_comments.push(comment),
                TokenKind::FileComment => file_comments.push(comment),
                _ => unreachable!(),
            }
        }
        line_trivia.sort_by_key(LineTrivia::start);

        Self {
            line_trivia,
            line_cursor: 0,
            doc_comments,
            doc_comments_cursor: 0,
            file_comments,
            source,
        }
    }

    fn newline_between(source: &str, start: u32, end: u32) -> bool {
        if start >= end {
            return false;
        }
        let s = start as usize;
        let e = (end as usize).min(source.len());
        source.as_bytes()[s..e].contains(&b'\n')
    }

    fn at_line_start(source: &str, at: u32) -> bool {
        let mut i = (at as usize).min(source.len());
        let bytes = source.as_bytes();
        while i > 0 {
            let b = bytes[i - 1];
            if b == b'\n' {
                return true;
            }
            if b != b' ' && b != b'\t' {
                return false;
            }
            i -= 1;
        }
        true
    }

    fn take_line_trivia_before(&mut self, before: u32) -> &[LineTrivia<'a>] {
        let start = self.line_cursor;
        let count = self.line_trivia[start..].partition_point(|event| event.start() < before);
        self.line_cursor += count;
        &self.line_trivia[start..self.line_cursor]
    }

    fn take_split_before(
        &mut self,
        before: u32,
        mut is_split_point: impl FnMut(u32) -> bool,
    ) -> SplitComments<'a> {
        let events = self.take_line_trivia_before(before);
        let split_at = events
            .iter()
            .position(|event| match event {
                LineTrivia::Comment(comment) => is_split_point(comment.start),
                LineTrivia::BlankLine(_) => true,
            })
            .unwrap_or(events.len());
        let (trailing, leading) = events.split_at(split_at);

        SplitComments {
            trailing: line_trivia_to_document(trailing),
            leading: line_trivia_to_document(leading),
            has_blank_before_leading: matches!(leading.first(), Some(LineTrivia::BlankLine(_))),
        }
    }

    pub fn take_split_at_line_start(&mut self, before: u32) -> SplitComments<'a> {
        let source = self.source;
        self.take_split_before(before, |start| Self::at_line_start(source, start))
    }

    pub fn take_split_by_newline_after(&mut self, anchor: u32, before: u32) -> SplitComments<'a> {
        let source = self.source;
        self.take_split_before(before, |start| Self::newline_between(source, anchor, start))
    }

    pub fn take_comments_before(&mut self, position: u32) -> Option<Document<'a>> {
        let events = self.take_line_trivia_before(position);
        line_trivia_to_document(events)
    }

    pub(crate) fn take_comments_and_blank_lines_before(
        &mut self,
        position: u32,
    ) -> TakenComments<'a> {
        let events = self.take_line_trivia_before(position);
        let has_blank_line = events
            .iter()
            .any(|event| matches!(event, LineTrivia::BlankLine(_)));
        let end = events
            .iter()
            .rposition(|event| matches!(event, LineTrivia::Comment(_)))
            .map_or(0, |index| index + 1);

        TakenComments {
            document: line_trivia_to_document(&events[..end]),
            has_blank_line,
        }
    }

    pub fn take_doc_comments_before(&mut self, position: u32) -> Option<Document<'a>> {
        let end = self.doc_comments[self.doc_comments_cursor..]
            .iter()
            .position(|c| c.start >= position)
            .map(|i| self.doc_comments_cursor + i)
            .unwrap_or(self.doc_comments.len());

        let popped = &self.doc_comments[self.doc_comments_cursor..end];
        self.doc_comments_cursor = end;

        doc_comment_to_document(popped.iter().map(|c| c.content))
    }

    pub fn take_file_comments(&mut self) -> Option<Document<'a>> {
        if self.file_comments.is_empty() {
            return None;
        }

        Some(join(
            self.file_comments
                .drain(..)
                .map(|c| Document::string(format!("//!{}", c.content))),
            Document::Newline,
        ))
    }

    pub fn take_trailing_comments(&mut self) -> Option<Document<'a>> {
        self.take_comments_before(u32::MAX)
    }

    pub(crate) fn has_comment_immediately_before(&self, position: u32) -> bool {
        let Some(comment) = self.line_trivia[self.line_cursor..]
            .iter()
            .filter_map(|event| match event {
                LineTrivia::Comment(comment) if comment.start < position => Some(comment),
                _ => None,
            })
            .next_back()
        else {
            return false;
        };
        self.source
            .get(comment.start as usize + 2 + comment.content.len()..position as usize)
            .is_some_and(|between| between.chars().all(char::is_whitespace))
    }

    pub(crate) fn source_slice(&self, offset: u32, length: u32) -> &'a str {
        let start = (offset as usize).min(self.source.len());
        let end = (start + length as usize).min(self.source.len());
        &self.source[start..end]
    }

    /// Source-scans for `needle needle` (e.g. `..`) in `[start, before)`, skipping comment text.
    pub(crate) fn next_pair_at(&self, needle: u8, start: u32, before: u32) -> Option<u32> {
        let bytes = self.source.as_bytes();
        let mut i = (start as usize).min(self.source.len());
        let e = (before as usize).min(self.source.len());
        let mut comment_idx = self.first_comment_overlapping(i);
        while let Some(pos) = self.scan_byte(bytes, &mut i, e, &mut comment_idx, needle) {
            let p = pos as usize;
            if p + 1 < e && bytes[p + 1] == needle {
                return Some(pos);
            }
            i = p + 1;
        }
        None
    }

    /// Source-scans for `needle` in `[start, before)`, skipping comment text.
    pub(crate) fn next_byte_at(&self, needle: u8, start: u32, before: u32) -> Option<u32> {
        let bytes = self.source.as_bytes();
        let mut i = (start as usize).min(self.source.len());
        let e = (before as usize).min(self.source.len());
        let mut comment_idx = self.first_comment_overlapping(i);
        self.scan_byte(bytes, &mut i, e, &mut comment_idx, needle)
    }

    fn first_comment_overlapping(&self, pos: usize) -> usize {
        // Use partition_point on sorted comments rather than linear scan.
        self.line_trivia.partition_point(|event| match event {
            LineTrivia::Comment(comment) => {
                comment.start as usize + 2 + comment.content.len() <= pos
            }
            LineTrivia::BlankLine(start) => *start as usize <= pos,
        })
    }

    fn scan_byte(
        &self,
        bytes: &[u8],
        i: &mut usize,
        e: usize,
        comment_idx: &mut usize,
        needle: u8,
    ) -> Option<u32> {
        while *i < e {
            while *comment_idx < self.line_trivia.len() {
                match &self.line_trivia[*comment_idx] {
                    LineTrivia::BlankLine(_) => *comment_idx += 1,
                    LineTrivia::Comment(comment) if comment.start as usize <= *i => {
                        *i = (comment.start as usize + 2 + comment.content.len()).min(e);
                        *comment_idx += 1;
                    }
                    LineTrivia::Comment(_) => break,
                }
            }
            if bytes[*i] == needle {
                return Some(*i as u32);
            }
            *i += 1;
        }
        None
    }

    pub fn has_comments_in_range(&self, span: syntax::ast::Span) -> bool {
        let start = span.byte_offset;
        let end = span.byte_offset + span.byte_length;

        self.line_trivia[self.line_cursor..]
            .iter()
            .any(|event| {
                matches!(event, LineTrivia::Comment(comment) if comment.start >= start && comment.start < end)
            })
    }
}

fn line_trivia_to_document<'a>(events: &[LineTrivia<'a>]) -> Option<Document<'a>> {
    let mut docs: Vec<Document<'a>> = Vec::new();
    let mut events = events
        .iter()
        .skip_while(|event| matches!(event, LineTrivia::BlankLine(_)))
        .peekable();

    while let Some(event) = events.next() {
        let LineTrivia::Comment(comment) = event else {
            continue;
        };
        docs.push(Document::string(format!("//{}", comment.content)));

        let mut has_blank_line = false;
        while matches!(events.peek(), Some(LineTrivia::BlankLine(_))) {
            has_blank_line = true;
            events.next();
        }
        if has_blank_line {
            docs.push(Document::Newline);
            if events.peek().is_some() {
                docs.push(Document::Newline);
            }
        } else if events.peek().is_some() {
            docs.push(Document::Newline);
        }
    }

    if docs.is_empty() {
        return None;
    }
    Some(concat(docs))
}

fn doc_comment_to_document<'a>(
    doc_comments: impl Iterator<Item = &'a str>,
) -> Option<Document<'a>> {
    let docs: Vec<_> = doc_comments
        .map(|c| Document::string(format!("///{c}")))
        .collect();

    if docs.is_empty() {
        return None;
    }

    Some(join(docs, Document::Newline))
}

pub fn prepend_comments<'a>(doc: Document<'a>, comments: Option<Document<'a>>) -> Document<'a> {
    match comments {
        Some(c) => c
            .append(Document::Newline)
            .force_break()
            .append(doc.group()),
        None => doc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comments<'a>(
        source: &'a str,
        ranges: Vec<(u32, u32)>,
        blank_lines: Vec<u32>,
    ) -> Comments<'a> {
        let comments = ranges
            .into_iter()
            .map(|(start, end)| Comment {
                start,
                content: &source[start as usize + 2..end as usize],
            })
            .map(LineTrivia::Comment)
            .chain(blank_lines.into_iter().map(LineTrivia::BlankLine))
            .collect::<Vec<_>>();
        let mut line_trivia = comments;
        line_trivia.sort_by_key(LineTrivia::start);
        Comments {
            line_trivia,
            line_cursor: 0,
            doc_comments: Vec::new(),
            doc_comments_cursor: 0,
            file_comments: Vec::new(),
            source,
        }
    }

    fn render(doc: Option<Document<'_>>) -> Option<String> {
        doc.map(|d| d.to_pretty_string(80))
    }

    #[test]
    fn take_split_at_line_start_no_comments_returns_none() {
        let source = "fn f() {}";
        let mut c = comments(source, Vec::new(), Vec::new());
        let split = c.take_split_at_line_start(100);
        assert_eq!(render(split.trailing), None);
        assert_eq!(render(split.leading), None);
        assert!(!split.has_blank_before_leading);
    }

    #[test]
    fn take_split_at_line_start_routes_same_line_vs_standalone() {
        let source = "x // a\n  // b\n y";
        let mut c = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let split = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(split.trailing).as_deref(), Some("// a"));
        assert_eq!(render(split.leading).as_deref(), Some("// b"));
        assert!(!split.has_blank_before_leading);
    }

    #[test]
    fn take_split_at_line_start_blank_before_new_line_sets_has_blank_above() {
        let source = "x // a\n\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (10, 14)], vec![7]);
        let split = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(split.trailing).as_deref(), Some("// a"));
        assert_eq!(render(split.leading).as_deref(), Some("// b"));
        assert!(split.has_blank_before_leading);
    }

    #[test]
    fn take_split_at_line_start_blank_between_new_line_entries_preserves_separator() {
        let source = "  // a\n\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (10, 14)], vec![7]);
        let split = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(split.trailing), None);
        let new_str = render(split.leading).expect("new_line should have content");
        assert!(new_str.contains("// a"));
        assert!(new_str.contains("// b"));
        assert!(new_str.contains("\n\n"));
        assert!(!split.has_blank_before_leading);
    }

    #[test]
    fn take_split_at_line_start_all_same_line() {
        let source = "a // 1 // 2";
        let mut c = comments(source, vec![(2, 6), (7, 11)], Vec::new());
        let split = c.take_split_at_line_start(source.len() as u32);
        let same_str = render(split.trailing).expect("same_line should have content");
        assert!(same_str.contains("// 1"));
        assert!(same_str.contains("// 2"));
        assert_eq!(render(split.leading), None);
        assert!(!split.has_blank_before_leading);
    }

    #[test]
    fn take_split_at_line_start_advances_cursor() {
        let source = "x // a\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let first = c.take_split_at_line_start(7);
        assert_eq!(render(first.trailing).as_deref(), Some("// a"));
        assert_eq!(render(first.leading), None);
        let second = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(second.trailing), None);
        assert_eq!(render(second.leading).as_deref(), Some("// b"));
    }

    #[test]
    fn take_split_at_line_start_respects_before_bound() {
        let source = "// a\n// b\n";
        let mut c = comments(source, vec![(0, 4), (5, 9)], Vec::new());
        let first = c.take_split_at_line_start(5);
        assert_eq!(render(first.leading).as_deref(), Some("// a"));
        let second = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(second.leading).as_deref(), Some("// b"));
    }

    #[test]
    fn take_split_by_newline_after_classifier_uses_anchor() {
        let source = "x // a\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let split = c.take_split_by_newline_after(2, source.len() as u32);
        assert_eq!(render(split.trailing).as_deref(), Some("// a"));
        assert_eq!(render(split.leading).as_deref(), Some("// b"));
    }
}
