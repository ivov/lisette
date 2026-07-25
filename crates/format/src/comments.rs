use crate::lindig::{Document, concat, join};
use syntax::lex::{Token, TokenKind};

#[derive(Debug, Clone)]
pub struct Comment<'a> {
    pub start: u32,
    pub content: &'a str,
}

pub struct Comments<'a> {
    comments: Vec<Comment<'a>>,
    doc_comments: Vec<Comment<'a>>,
    file_comments: Vec<Comment<'a>>,
    empty_lines: Vec<u32>,
    cursor: Cursor,
    source: &'a str,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Cursor {
    comments: usize,
    doc_comments: usize,
    empty_lines: usize,
}

impl<'a> Comments<'a> {
    pub fn from_lexed(tokens: &[Token<'_>], blank_lines: Vec<u32>, source: &'a str) -> Self {
        let mut comments = Vec::new();
        let mut doc_comments = Vec::new();
        let mut file_comments = Vec::new();
        for token in tokens {
            let (prefix, destination) = match token.kind {
                TokenKind::Comment => ("//", &mut comments),
                TokenKind::DocComment => ("///", &mut doc_comments),
                TokenKind::FileComment => ("//!", &mut file_comments),
                _ => continue,
            };
            let start = token.byte_offset;
            let content = source
                .get(start as usize..token.end_offset() as usize)
                .expect("comment token must be a valid source range")
                .strip_prefix(prefix)
                .expect("comment token text must match its kind");
            destination.push(Comment { start, content });
        }

        Self {
            comments,
            doc_comments,
            file_comments,
            empty_lines: blank_lines,
            cursor: Cursor::default(),
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

    pub(crate) fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub(crate) fn restore_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }

    /// Drains comments before `before`; returns `(same_line, new_line, has_blank_above)`.
    fn take_split_before(
        &mut self,
        before: u32,
        mut is_split_point: impl FnMut(u32) -> bool,
    ) -> (Option<Document<'a>>, Option<Document<'a>>, bool) {
        let comment_end = self.comments[self.cursor.comments..]
            .iter()
            .position(|c| c.start >= before)
            .map(|i| self.cursor.comments + i)
            .unwrap_or(self.comments.len());
        let popped_comments = &self.comments[self.cursor.comments..comment_end];
        self.cursor.comments = comment_end;

        let empty_end = self.empty_lines[self.cursor.empty_lines..]
            .iter()
            .position(|&l| l >= before)
            .map(|i| self.cursor.empty_lines + i)
            .unwrap_or(self.empty_lines.len());
        let popped_empty = &self.empty_lines[self.cursor.empty_lines..empty_end];
        self.cursor.empty_lines = empty_end;

        let comments_iter = popped_comments.iter().map(|c| (c.start, Some(c.content)));
        let empty_iter = popped_empty.iter().map(|&pos| (pos, None));
        let mut events: Vec<_> = comments_iter.chain(empty_iter).collect();
        events.sort_by_key(|(pos, _)| *pos);

        let mut same_line: Vec<Option<&'a str>> = Vec::new();
        let mut new_line: Vec<Option<&'a str>> = Vec::new();
        let mut split_seen = false;
        let mut new_line_has_some = false;
        let mut has_blank_above = false;

        for (pos, content) in events {
            if !split_seen && content.is_some() && is_split_point(pos) {
                split_seen = true;
            }
            if content.is_none() {
                split_seen = true;
                if new_line_has_some {
                    new_line.push(None);
                } else {
                    has_blank_above = true;
                }
            } else if split_seen {
                new_line.push(content);
                new_line_has_some = true;
            } else {
                same_line.push(content);
            }
        }

        (
            comments_to_document(same_line),
            comments_to_document(new_line),
            has_blank_above,
        )
    }

    pub fn take_split_at_line_start(
        &mut self,
        before: u32,
    ) -> (Option<Document<'a>>, Option<Document<'a>>, bool) {
        let source = self.source;
        self.take_split_before(before, |start| Self::at_line_start(source, start))
    }

    pub fn take_split_by_newline_after(
        &mut self,
        anchor: u32,
        before: u32,
    ) -> (Option<Document<'a>>, Option<Document<'a>>, bool) {
        let source = self.source;
        self.take_split_before(before, |start| Self::newline_between(source, anchor, start))
    }

    pub fn take_comments_before(&mut self, position: u32) -> Option<Document<'a>> {
        let comment_end = self.comments[self.cursor.comments..]
            .iter()
            .position(|c| c.start >= position)
            .map(|i| self.cursor.comments + i)
            .unwrap_or(self.comments.len());

        let empty_end = self.empty_lines[self.cursor.empty_lines..]
            .iter()
            .position(|&l| l >= position)
            .map(|i| self.cursor.empty_lines + i)
            .unwrap_or(self.empty_lines.len());

        let popped_comments = &self.comments[self.cursor.comments..comment_end];
        let popped_empty = &self.empty_lines[self.cursor.empty_lines..empty_end];

        self.cursor.comments = comment_end;
        self.cursor.empty_lines = empty_end;

        let comments_iter = popped_comments.iter().map(|c| (c.start, Some(c.content)));
        let empty_iter = popped_empty.iter().map(|&position| (position, None));

        let mut all: Vec<_> = comments_iter.chain(empty_iter).collect();
        all.sort_by_key(|(position, _)| *position);

        let merged: Vec<_> = all
            .into_iter()
            .skip_while(|(_, c)| c.is_none())
            .map(|(_, c)| c)
            .collect();

        comments_to_document(merged)
    }

    pub fn take_doc_comments_before(&mut self, position: u32) -> Option<Document<'a>> {
        let end = self.doc_comments[self.cursor.doc_comments..]
            .iter()
            .position(|c| c.start >= position)
            .map(|i| self.cursor.doc_comments + i)
            .unwrap_or(self.doc_comments.len());

        let popped = &self.doc_comments[self.cursor.doc_comments..end];
        self.cursor.doc_comments = end;

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

    pub fn take_empty_lines_before(&mut self, position: u32) -> bool {
        let end = self.empty_lines[self.cursor.empty_lines..]
            .iter()
            .position(|&l| l >= position)
            .map(|i| self.cursor.empty_lines + i)
            .unwrap_or(self.empty_lines.len());

        let found = end > self.cursor.empty_lines;
        self.cursor.empty_lines = end;
        found
    }

    pub fn has_comments_before(&self, position: u32) -> bool {
        self.comments[self.cursor.comments..]
            .first()
            .is_some_and(|c| c.start < position)
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
        self.comments
            .partition_point(|c| (c.start as usize) + 2 + c.content.len() <= pos)
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
            if *comment_idx < self.comments.len() {
                let c = &self.comments[*comment_idx];
                let cs = c.start as usize;
                if cs <= *i {
                    *i = (cs + 2 + c.content.len()).min(e);
                    *comment_idx += 1;
                    continue;
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

        self.comments[self.cursor.comments..]
            .iter()
            .any(|c| c.start >= start && c.start < end)
    }
}

fn comments_to_document<'a>(comments: Vec<Option<&'a str>>) -> Option<Document<'a>> {
    let mut comments = comments.into_iter().peekable();
    let _ = comments.peek()?;

    let mut docs: Vec<Document<'a>> = Vec::new();

    while let Some(c) = comments.next() {
        let c = match c {
            Some(c) => c,
            None => continue,
        };

        docs.push(Document::string(format!("//{c}")));

        match comments.peek() {
            Some(Some(_)) => docs.push(Document::Newline),
            Some(None) => {
                let _ = comments.next();
                docs.push(Document::Newline);
                if comments.peek().is_some() {
                    docs.push(Document::Newline);
                }
            }
            None => {}
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
            .collect::<Vec<_>>();
        Comments {
            comments,
            doc_comments: Vec::new(),
            file_comments: Vec::new(),
            empty_lines: blank_lines,
            cursor: Cursor::default(),
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
        let (same, new, has_blank) = c.take_split_at_line_start(100);
        assert_eq!(render(same), None);
        assert_eq!(render(new), None);
        assert!(!has_blank);
    }

    #[test]
    fn take_split_at_line_start_routes_same_line_vs_standalone() {
        let source = "x // a\n  // b\n y";
        let mut c = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let (same, new, has_blank) = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(same).as_deref(), Some("// a"));
        assert_eq!(render(new).as_deref(), Some("// b"));
        assert!(!has_blank);
    }

    #[test]
    fn take_split_at_line_start_blank_before_new_line_sets_has_blank_above() {
        let source = "x // a\n\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (10, 14)], vec![7]);
        let (same, new, has_blank) = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(same).as_deref(), Some("// a"));
        assert_eq!(render(new).as_deref(), Some("// b"));
        assert!(has_blank);
    }

    #[test]
    fn take_split_at_line_start_blank_between_new_line_entries_preserves_separator() {
        let source = "  // a\n\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (10, 14)], vec![7]);
        let (same, new, has_blank) = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(same), None);
        let new_str = render(new).expect("new_line should have content");
        assert!(new_str.contains("// a"));
        assert!(new_str.contains("// b"));
        assert!(new_str.contains("\n\n"));
        assert!(!has_blank);
    }

    #[test]
    fn take_split_at_line_start_all_same_line() {
        let source = "a // 1 // 2";
        let mut c = comments(source, vec![(2, 6), (7, 11)], Vec::new());
        let (same, new, has_blank) = c.take_split_at_line_start(source.len() as u32);
        let same_str = render(same).expect("same_line should have content");
        assert!(same_str.contains("// 1"));
        assert!(same_str.contains("// 2"));
        assert_eq!(render(new), None);
        assert!(!has_blank);
    }

    #[test]
    fn take_split_at_line_start_advances_cursor() {
        let source = "x // a\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let (same1, new1, _) = c.take_split_at_line_start(7);
        assert_eq!(render(same1).as_deref(), Some("// a"));
        assert_eq!(render(new1), None);
        let (same2, new2, _) = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(same2), None);
        assert_eq!(render(new2).as_deref(), Some("// b"));
    }

    #[test]
    fn take_split_at_line_start_respects_before_bound() {
        let source = "// a\n// b\n";
        let mut c = comments(source, vec![(0, 4), (5, 9)], Vec::new());
        let (_, new, _) = c.take_split_at_line_start(5);
        assert_eq!(render(new).as_deref(), Some("// a"));
        let (_, new2, _) = c.take_split_at_line_start(source.len() as u32);
        assert_eq!(render(new2).as_deref(), Some("// b"));
    }

    #[test]
    fn take_split_by_newline_after_classifier_uses_anchor() {
        let source = "x // a\n  // b\n";
        let mut c = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let (same, new, _) = c.take_split_by_newline_after(2, source.len() as u32);
        assert_eq!(render(same).as_deref(), Some("// a"));
        assert_eq!(render(new).as_deref(), Some("// b"));
    }
}
