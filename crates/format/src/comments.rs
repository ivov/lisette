use crate::lindig::{Document, concat, join};
use syntax::lex::Trivia;

#[derive(Debug, Clone)]
pub struct Comment<'a> {
    pub start: u32,
    pub content: &'a str,
}

pub struct Comments<'a> {
    comments: Vec<Comment<'a>>,
    comments_cursor: usize,

    doc_comments: Vec<Comment<'a>>,
    doc_comments_cursor: usize,

    empty_lines: &'a [u32],
    empty_cursor: usize,
}

impl<'a> Comments<'a> {
    pub fn from_trivia(trivia: &'a Trivia, source: &'a str) -> Self {
        let comments = trivia
            .comments
            .iter()
            .filter_map(|&(start, end)| {
                let content = source.get(start as usize..end as usize)?;
                let content = content.strip_prefix("//").unwrap_or(content);
                Some(Comment { start, content })
            })
            .collect();

        let doc_comments = trivia
            .doc_comments
            .iter()
            .filter_map(|&(start, end)| {
                let content = source.get(start as usize..end as usize)?;
                let content = content.strip_prefix("///").unwrap_or(content);
                Some(Comment { start, content })
            })
            .collect();

        Self {
            comments,
            comments_cursor: 0,
            doc_comments,
            doc_comments_cursor: 0,
            empty_lines: &trivia.blank_lines,
            empty_cursor: 0,
        }
    }

    pub fn take_comments_before(&mut self, position: u32) -> Option<Document<'a>> {
        let comment_end = self.comments[self.comments_cursor..]
            .iter()
            .position(|c| c.start >= position)
            .map(|i| self.comments_cursor + i)
            .unwrap_or(self.comments.len());

        let empty_end = self.empty_lines[self.empty_cursor..]
            .iter()
            .position(|&l| l >= position)
            .map(|i| self.empty_cursor + i)
            .unwrap_or(self.empty_lines.len());

        let popped_comments = &self.comments[self.comments_cursor..comment_end];
        let popped_empty = &self.empty_lines[self.empty_cursor..empty_end];

        self.comments_cursor = comment_end;
        self.empty_cursor = empty_end;

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
        let end = self.doc_comments[self.doc_comments_cursor..]
            .iter()
            .position(|c| c.start >= position)
            .map(|i| self.doc_comments_cursor + i)
            .unwrap_or(self.doc_comments.len());

        let popped = &self.doc_comments[self.doc_comments_cursor..end];
        self.doc_comments_cursor = end;

        doc_comment_to_document(popped.iter().map(|c| c.content))
    }

    pub fn take_trailing_comments(&mut self) -> Option<Document<'a>> {
        self.take_comments_before(u32::MAX)
    }

    pub fn take_empty_lines_before(&mut self, position: u32) -> bool {
        let end = self.empty_lines[self.empty_cursor..]
            .iter()
            .position(|&l| l >= position)
            .map(|i| self.empty_cursor + i)
            .unwrap_or(self.empty_lines.len());

        let found = end > self.empty_cursor;
        self.empty_cursor = end;
        found
    }

    pub fn has_comments_in_range(&self, span: syntax::ast::Span) -> bool {
        let start = span.byte_offset;
        let end = span.byte_offset + span.byte_length;

        self.comments[self.comments_cursor..]
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
